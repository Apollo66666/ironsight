//! Export raw Mevo+ radar points and GVP camera tracker points to local files.
//!
//! Windows example:
//! cargo run --release --features gvp --example raw_point_export -- \
//!   --device 192.168.2.1:5100 --mode indoor \
//!   --range-mm 2743 --height-mm 25 --output C:\\MevoData
//!
//! The exporter merges device-pushed pages with an active 0xEC/0xEE re-fetch
//! performed by `BinaryClient` before the device is re-armed.

use std::collections::{HashMap, HashSet};
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ironsight::client::{BinaryClient, BinaryEvent};
use ironsight::conn::DEFAULT_ADDR;
use ironsight::gvp::client::GvpClient;
use ironsight::gvp::config::GvpConfig;
use ironsight::gvp::conn::DEFAULT_PORT;
use ironsight::gvp::result::{BallTrackerResult, Track};
use ironsight::gvp::track::ExpectedTrack;
use ironsight::gvp::trigger::Trigger;
use ironsight::gvp::{GvpConnection, GvpError, GvpEvent, GvpMessage};
use ironsight::protocol::camera::{CamConfig, CamConfigReq, CamState};
use ironsight::protocol::config::{MODE_INDOOR, MODE_OUTDOOR, ParamData, ParamValue, RadarCal};
use ironsight::protocol::shot::{
    ClubPrc, ClubResult, FlightResult, FlightResultV1, PrcData, SpeedProfile, SpinResult,
    TrackingStatus,
};
use ironsight::seq::{self, AvrSettings, PrcFetchStatus, ShotData, ShotDatum};
use ironsight::{BinaryConnection, BusAddr, Command, ConnError, Message};
use serde_json::{Value, json};

type AppResult<T> = Result<T, Box<dyn std::error::Error>>;

const SCHEMA_VERSION: u8 = 1;
const CAM_FX: f64 = 500.0;
const CAM_FY: f64 = 500.0;
const CAM_CX: f64 = 320.0;
const CAM_CY: f64 = 240.0;
const CAM_HEIGHT: f64 = 0.10;
const CAM_TILT_DEG: f64 = 12.5;
const BALL_RADIUS_M: f64 = 0.021;
const CLUB_RADIUS_M: f64 = 0.050;
const N_SAMPLES: usize = 21;
const TRACK_DURATION: f64 = 0.1;
const DEFAULT_START_TIME: f64 = 0.014;

#[derive(Debug)]
struct Options {
    device: SocketAddr,
    mode_name: String,
    mode: u8,
    range_mm: u16,
    height_mm: u8,
    output: PathBuf,
}

impl Options {
    fn parse() -> AppResult<Self> {
        let default_output = if cfg!(windows) {
            PathBuf::from(r"C:\MevoData")
        } else {
            PathBuf::from("./MevoData")
        };
        let mut options = Self {
            device: DEFAULT_ADDR.parse()?,
            mode_name: "indoor".to_owned(),
            mode: MODE_INDOOR,
            range_mm: 2743,
            height_mm: 25,
            output: default_output,
        };

        let mut args = env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--device" => {
                    let value = required_value(&mut args, "--device")?;
                    options.device = parse_device(&value)?;
                }
                "--mode" => {
                    let value = required_value(&mut args, "--mode")?;
                    let (name, mode) = match value.to_ascii_lowercase().as_str() {
                        "indoor" => ("indoor", MODE_INDOOR),
                        "outdoor" => ("outdoor", MODE_OUTDOOR),
                        _ => return Err("--mode must be indoor or outdoor".into()),
                    };
                    options.mode_name = name.to_owned();
                    options.mode = mode;
                }
                "--range-mm" => {
                    options.range_mm = required_value(&mut args, "--range-mm")?.parse()?;
                    if options.range_mm == 0 {
                        return Err("--range-mm must be greater than zero".into());
                    }
                }
                "--height-mm" => {
                    options.height_mm = required_value(&mut args, "--height-mm")?.parse()?;
                }
                "--output" => {
                    options.output = PathBuf::from(required_value(&mut args, "--output")?);
                }
                "--help" | "-h" => {
                    print_usage();
                    process::exit(0);
                }
                _ => return Err(format!("unknown argument: {arg}").into()),
            }
        }
        Ok(options)
    }
}

fn required_value(args: &mut impl Iterator<Item = String>, flag: &str) -> AppResult<String> {
    args.next()
        .ok_or_else(|| format!("missing value after {flag}").into())
}

fn parse_device(value: &str) -> AppResult<SocketAddr> {
    let with_port = if value.contains(':') {
        value.to_owned()
    } else {
        format!("{value}:5100")
    };
    Ok(with_port.parse()?)
}

fn print_usage() {
    println!(
        "raw_point_export [options]\n\
         \n\
         --device <ip[:port]>  Mevo+ binary endpoint (default {DEFAULT_ADDR})\n\
         --mode <name>         indoor or outdoor (default indoor)\n\
         --range-mm <number>   radar-to-ball distance in mm (default 2743)\n\
         --height-mm <number>  radar height in mm (default 25)\n\
         --output <directory>  session parent directory (default C:\\MevoData on Windows)\n"
    );
}

#[derive(Default)]
struct ShotCollector {
    local_shot_id: u64,
    guid: String,
    trigger_epoch: f64,
    flight: Option<FlightResult>,
    flight_v1: Option<FlightResultV1>,
    club: Option<ClubResult>,
    spin: Option<SpinResult>,
    speed_profile: Option<SpeedProfile>,
    tracking_status: Vec<TrackingStatus>,
    ball_prc_pages: Vec<PrcData>,
    club_prc_pages: Vec<ClubPrc>,
    prc_fetch: PrcFetchStatus,
    camera_result: Option<BallTrackerResult>,
    gvp_result_raw: Option<String>,
    shot_lifecycle_complete: bool,
    radar_complete: bool,
    hints_sent: bool,
    warnings: Vec<String>,
}

impl ShotCollector {
    fn new(local_shot_id: u64, guid: String, trigger_epoch: f64) -> Self {
        Self {
            local_shot_id,
            guid,
            trigger_epoch,
            ..Self::default()
        }
    }

    fn add_warning(&mut self, warning: impl Into<String>) {
        let warning = warning.into();
        if !self.warnings.contains(&warning) {
            self.warnings.push(warning);
        }
    }

    fn merge_message(&mut self, message: &Message) {
        match message {
            Message::FlightResult(value) => self.flight = Some(value.clone()),
            Message::FlightResultV1(value) => self.flight_v1 = Some(value.clone()),
            Message::ClubResult(value) => self.club = Some(value.clone()),
            Message::SpinResult(value) => self.spin = Some(value.clone()),
            Message::SpeedProfile(value) => self.speed_profile = Some(value.clone()),
            Message::TrackingStatus(value) => self.tracking_status.push(value.clone()),
            Message::PrcData(value) => self.ball_prc_pages.push(value.clone()),
            Message::ClubPrc(value) => self.club_prc_pages.push(value.clone()),
            _ => {}
        }
    }

    fn merge_datum(&mut self, datum: &ShotDatum) {
        match datum {
            ShotDatum::Flight(value) => self.flight = Some(value.clone()),
            ShotDatum::FlightV1(value) => self.flight_v1 = Some(value.clone()),
            ShotDatum::Club(value) => self.club = Some(value.clone()),
            ShotDatum::Spin(value) => self.spin = Some(value.clone()),
        }
    }

    fn merge_complete(&mut self, data: &ShotData) {
        if let Some(value) = &data.flight {
            self.flight = Some(value.clone());
        }
        if let Some(value) = &data.club {
            self.club = Some(value.clone());
        }
        if let Some(value) = &data.spin {
            self.spin = Some(value.clone());
        }
        if let Some(value) = &data.speed_profile {
            self.speed_profile = Some(value.clone());
        }
        self.ball_prc_pages.extend(data.prc.iter().cloned());
        self.club_prc_pages.extend(data.club_prc.iter().cloned());
        self.prc_fetch = data.prc_fetch.clone();
        self.shot_lifecycle_complete = true;
        self.refresh_radar_complete();
    }

    fn refresh_radar_complete(&mut self) {
        let ball_count = unique_ball_points(self).len();
        let club_count = unique_club_points(self).len();
        let expected_ball = self
            .tracking_status
            .iter()
            .map(|status| usize::from(status.prc_tracking_count))
            .max()
            .unwrap_or(0)
            .max(self.prc_fetch.ball_expected_points);
        let expected_club = self
            .club
            .as_ref()
            .map_or(0, |club| usize::from(club.num_club_prc_points))
            .max(self.prc_fetch.club_expected_points);
        let ball_count_ok = ball_count > 0 && (expected_ball == 0 || ball_count >= expected_ball);
        let club_count_ok = expected_club == 0 || club_count >= expected_club;
        let pagination_ok = !self.prc_fetch.enabled
            || (self.prc_fetch.ball_complete && self.prc_fetch.club_complete);
        self.radar_complete = self.shot_lifecycle_complete
            && (self.flight.is_some() || self.flight_v1.is_some())
            && ball_count_ok
            && club_count_ok
            && pagination_ok;
    }
}

struct SessionInfo {
    started_epoch: f64,
    session_dir: PathBuf,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        process::exit(1);
    }
}

fn run() -> AppResult<()> {
    let options = Options::parse()?;
    let started_epoch = epoch_now();
    let session_dir = create_session_directory(&options.output, started_epoch)?;
    let session = SessionInfo {
        started_epoch,
        session_dir,
    };
    touch(&session.session_dir.join("session.log"))?;
    log_event(
        &session.session_dir,
        &format!("session started; options={options:?}"),
    );
    write_session_json(&session, &options, 0, 0, 0)?;

    let binary_addr = options.device;
    let gvp_addr: SocketAddr = format!("{}:{DEFAULT_PORT}", binary_addr.ip()).parse()?;

    println!("Output session: {}", session.session_dir.display());
    println!("Connecting to binary protocol at {binary_addr}...");
    let mut conn = BinaryConnection::connect_timeout(&binary_addr, Duration::from_secs(5))?;
    println!("Connected to binary protocol.");

    println!("DSP sync...");
    let dsp = seq::sync_dsp(&mut conn)?;
    println!("AVR sync...");
    let avr = seq::sync_avr(&mut conn)?;
    println!("PI sync...");
    let pi = seq::sync_pi(&mut conn)?;
    write_device_json(&session.session_dir, &dsp, &avr, &pi)?;
    log_event(
        &session.session_dir,
        &format!(
            "handshake complete; device={}; dsp={}; avr={}; pi={}",
            dsp.hw_info.device_gen(),
            dsp.dev_info.text,
            avr.dev_info.text,
            pi.dev_info.text
        ),
    );

    let settings = AvrSettings {
        mode: options.mode,
        params: vec![
            ParamValue {
                param_id: 0x06,
                value: ParamData::Int24(0),
            },
            ParamValue {
                param_id: 0x0F,
                value: ParamData::Float40(1.0),
            },
            ParamValue {
                param_id: 0x26,
                value: ParamData::Float40(0.0381),
            },
        ],
        radar_cal: Some(RadarCal {
            range_mm: options.range_mm,
            height_mm: options.height_mm,
        }),
    };

    println!(
        "Configuring mode={} range={}mm height={}mm...",
        options.mode_name, options.range_mm, options.height_mm
    );
    seq::configure_avr(&mut conn, &settings)?;

    println!("Starting camera (standard warmup)...");
    start_camera(&mut conn, &CamConfig::standard_preset(), "standard")?;
    println!("Switching camera to Raw Fusion...");
    start_camera(&mut conn, &CamConfig::raw_fusion_preset(), "raw-fusion")?;

    let mut binary = BinaryClient::from_tcp(conn)?;
    binary.set_prc_pagination_enabled(true);
    binary.arm();

    let raw_results: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
    println!("Connecting to GVP at {gvp_addr}...");
    let mut gvp_conn = GvpConnection::connect_timeout(&gvp_addr, Duration::from_secs(5))?;
    {
        let raw_results = Arc::clone(&raw_results);
        gvp_conn.set_on_recv(move |raw_json, message| {
            if let GvpMessage::Result(result) = message
                && let Ok(mut values) = raw_results.lock()
            {
                values.insert(result.guid.clone(), raw_json.to_owned());
            }
        });
    }
    let mut gvp = GvpClient::from_tcp(gvp_conn)?;
    println!("GVP connected. Waiting for shots...");
    log_event(&session.session_dir, "camera and GVP ready; arming radar");

    let range_m = f64::from(options.range_mm) / 1000.0;
    let mut shot_count = 0u64;
    let mut current_guid: Option<String> = None;
    let mut collectors: HashMap<String, ShotCollector> = HashMap::new();
    let mut gvp_connected = true;

    loop {
        if let Some(event) = binary.poll()? {
            match event {
                BinaryEvent::Armed => println!("ARMED - hit a ball."),
                BinaryEvent::Trigger => {
                    shot_count += 1;
                    let epoch = epoch_now();
                    let guid = new_guid(shot_count);
                    println!("Shot #{shot_count} triggered; guid={guid}");
                    log_event(
                        &session.session_dir,
                        &format!("shot {shot_count} trigger guid={guid}"),
                    );
                    collectors.insert(
                        guid.clone(),
                        ShotCollector::new(shot_count, guid.clone(), epoch),
                    );
                    current_guid = Some(guid.clone());

                    if gvp_connected {
                        if let Err(error) = gvp.send_trigger(&Trigger::new(guid.clone(), epoch))
                            && let Some(shot) = collectors.get_mut(&guid)
                        {
                            shot.add_warning(format!("发送 GVP Trigger 失败: {error}"));
                        }
                        let mut gvp_config = GvpConfig::fusion();
                        gvp_config.save_videos_enabled = false;
                        if let Err(error) = gvp.send_config(&gvp_config)
                            && let Some(shot) = collectors.get_mut(&guid)
                        {
                            shot.add_warning(format!("发送 GVP Config 失败: {error}"));
                        }
                    }
                }
                BinaryEvent::Message(envelope) => {
                    if let Some(guid) = current_guid.as_deref()
                        && let Some(shot) = collectors.get_mut(guid)
                    {
                        shot.merge_message(&envelope.message);
                        if matches!(envelope.message, Message::FlightResult(_)) {
                            try_send_hints(shot, &mut gvp, gvp_connected, range_m);
                        }
                    }
                }
                BinaryEvent::ShotDatum(datum) => {
                    if let Some(guid) = current_guid.as_deref()
                        && let Some(shot) = collectors.get_mut(guid)
                    {
                        shot.merge_datum(&datum);
                        if matches!(datum, ShotDatum::Flight(_)) {
                            try_send_hints(shot, &mut gvp, gvp_connected, range_m);
                        }
                    }
                }
                BinaryEvent::ShotComplete(data) => {
                    if let Some(guid) = current_guid.as_deref()
                        && let Some(shot) = collectors.get_mut(guid)
                    {
                        shot.merge_complete(&data);
                        try_send_hints(shot, &mut gvp, gvp_connected, range_m);
                        attach_raw_result(shot, &raw_results);
                        write_shot_outputs(&session.session_dir, shot)?;
                        println!(
                            "Shot #{} radar saved: ball={} club={} radarComplete={}",
                            shot.local_shot_id,
                            unique_ball_points(shot).len(),
                            unique_club_points(shot).len(),
                            shot.radar_complete
                        );
                        log_event(
                            &session.session_dir,
                            &format!("shot {} radar files saved", shot.local_shot_id),
                        );
                    }
                    write_session_from_collectors(&session, &options, &collectors)?;
                }
                BinaryEvent::Configured
                | BinaryEvent::Disarmed
                | BinaryEvent::Handshake(_)
                | BinaryEvent::Keepalive(_) => {}
            }
        }

        if gvp_connected {
            match gvp.poll() {
                Ok(Some(GvpEvent::Result(result))) => {
                    let guid = result.guid.clone();
                    if let Some(shot) = collectors.get_mut(&guid) {
                        shot.camera_result = Some(*result);
                        attach_raw_result(shot, &raw_results);
                        write_shot_outputs(&session.session_dir, shot)?;
                        println!("Shot #{} camera files saved.", shot.local_shot_id);
                        log_event(
                            &session.session_dir,
                            &format!("shot {} GVP result saved", shot.local_shot_id),
                        );
                        write_session_from_collectors(&session, &options, &collectors)?;
                    } else {
                        eprintln!("warning: GVP RESULT has unknown guid={guid}");
                        log_event(
                            &session.session_dir,
                            &format!("unmatched GVP RESULT guid={guid}"),
                        );
                    }
                }
                Ok(Some(GvpEvent::Status(status))) => {
                    println!("[gvp] status={}", status.status());
                }
                Ok(Some(GvpEvent::Log(message))) => {
                    println!("[gvp] {}: {}", message.level, message.message);
                }
                Ok(Some(
                    GvpEvent::Config(_) | GvpEvent::VideoAvailable(_) | GvpEvent::Unknown { .. },
                ))
                | Ok(None) => {}
                Err(GvpError::Disconnected) => {
                    gvp_connected = false;
                    eprintln!("warning: GVP disconnected; radar export will continue");
                    log_event(
                        &session.session_dir,
                        "GVP disconnected; continuing radar-only capture",
                    );
                }
                Err(error) => {
                    eprintln!("warning: GVP receive error: {error}");
                    log_event(&session.session_dir, &format!("GVP receive error: {error}"));
                }
            }
        }

        thread::sleep(Duration::from_millis(2));
    }
}

fn attach_raw_result(shot: &mut ShotCollector, raw_results: &Arc<Mutex<HashMap<String, String>>>) {
    if shot.gvp_result_raw.is_none()
        && let Ok(mut values) = raw_results.lock()
        && let Some(raw) = values.remove(&shot.guid)
    {
        shot.gvp_result_raw = Some(raw);
    }
}

fn try_send_hints(
    shot: &mut ShotCollector,
    gvp: &mut GvpClient<std::net::TcpStream>,
    gvp_connected: bool,
    range_m: f64,
) {
    if shot.hints_sent || !gvp_connected {
        return;
    }
    let Some(flight) = shot.flight.as_ref() else {
        return;
    };
    let start_time = shot
        .ball_prc_pages
        .iter()
        .flat_map(|page| page.points.first())
        .next()
        .map_or(DEFAULT_START_TIME, |point| f64::from(point.time) * 26.7e-6);
    let hints =
        compute_trajectory_hints(flight, shot.club.as_ref(), &shot.guid, range_m, start_time);
    if let Some(club_track) = hints.club_track
        && let Err(error) = gvp.send_club_track(&club_track)
    {
        shot.add_warning(format!("发送 Expected Club Track 失败: {error}"));
    }
    if let Err(error) = gvp.send_ball_track(&hints.ball_track) {
        shot.add_warning(format!("发送 Expected Ball Track 失败: {error}"));
    }
    shot.hints_sent = true;
}

struct TrajectoryHints {
    club_track: Option<ExpectedTrack>,
    ball_track: ExpectedTrack,
}

fn compute_trajectory_hints(
    flight: &FlightResult,
    club: Option<&ClubResult>,
    guid: &str,
    range_m: f64,
    start_time: f64,
) -> TrajectoryHints {
    let (ball_poly_u, ball_poly_v) = fit_track_polynomials(
        &flight.poly_x,
        &flight.poly_y,
        &flight.poly_z,
        range_m,
        start_time,
        TRACK_DURATION,
    );
    let ball_poly_r = make_poly_radius(&flight.poly_x, BALL_RADIUS_M, range_m, start_time);
    let ball_track = ExpectedTrack {
        guid: guid.to_owned(),
        duration: TRACK_DURATION,
        start_time,
        poly_u: ball_poly_u,
        poly_v: ball_poly_v,
        poly_radius: ball_poly_r,
    };

    let club_track = match club {
        Some(club) if club.dynamic_loft != 0.0 => {
            let cx = &club.poly_coeffs[2];
            let cy = &club.poly_coeffs[4];
            let cz = &club.poly_coeffs[6];
            let club_x = [cx[0], cx[1], cx[2], 0.0, 0.0];
            let club_y = [cy[0], cy[1], cy[2], 0.0, 0.0];
            let club_z = [cz[0], cz[1], cz[2], 0.0, 0.0];
            let (poly_u, poly_v) = fit_track_polynomials(
                &club_x,
                &club_y,
                &club_z,
                range_m,
                start_time,
                TRACK_DURATION,
            );
            Some(ExpectedTrack {
                guid: guid.to_owned(),
                duration: TRACK_DURATION,
                start_time,
                poly_u,
                poly_v,
                poly_radius: make_poly_radius(&club_x, CLUB_RADIUS_M, range_m, start_time),
            })
        }
        _ => Some(ExpectedTrack {
            guid: guid.to_owned(),
            duration: TRACK_DURATION,
            start_time,
            poly_u: ball_poly_u,
            poly_v: ball_poly_v,
            poly_radius: make_poly_radius(&flight.poly_x, CLUB_RADIUS_M, range_m, start_time),
        }),
    };

    TrajectoryHints {
        club_track,
        ball_track,
    }
}

fn eval_poly(coeffs: &[f64], t: f64) -> f64 {
    coeffs
        .iter()
        .rev()
        .fold(0.0, |result, coefficient| result * t + coefficient)
}

fn project_to_pixel(x: f64, y: f64, z: f64) -> (f64, f64) {
    let theta = CAM_TILT_DEG.to_radians();
    let (sin_t, cos_t) = theta.sin_cos();
    let dx = x;
    let dy = y - CAM_HEIGHT;
    let x_cam = (dx * cos_t + dy * sin_t).max(0.1);
    let y_cam = -dx * sin_t + dy * cos_t;
    (CAM_FX * z / x_cam + CAM_CX, CAM_CY - CAM_FY * y_cam / x_cam)
}

fn fit_track_polynomials(
    poly_x: &[f64],
    poly_y: &[f64],
    poly_z: &[f64],
    range_m: f64,
    start_time: f64,
    duration: f64,
) -> ([f64; 5], [f64; 5]) {
    let mut times = Vec::with_capacity(N_SAMPLES);
    let mut us = Vec::with_capacity(N_SAMPLES);
    let mut vs = Vec::with_capacity(N_SAMPLES);
    for index in 0..N_SAMPLES {
        let local_time = index as f64 * duration / (N_SAMPLES - 1) as f64;
        let real_time = start_time + local_time;
        let (u, v) = project_to_pixel(
            range_m + eval_poly(poly_x, real_time),
            eval_poly(poly_y, real_time),
            eval_poly(poly_z, real_time),
        );
        times.push(local_time);
        us.push(u);
        vs.push(v);
    }
    (poly_fit_4(&times, &us), poly_fit_4(&times, &vs))
}

fn make_poly_radius(poly_x: &[f64], object_radius: f64, range_m: f64, start_time: f64) -> [f64; 5] {
    let x0 = (range_m + eval_poly(poly_x, start_time)).max(0.1);
    let radius = CAM_FX * object_radius / x0;
    [1.0, radius, radius * 0.15, 0.0, 0.0]
}

#[allow(clippy::needless_range_loop)]
fn solve_5x5(a: &mut [[f64; 5]; 5], b: &mut [f64; 5]) -> [f64; 5] {
    for column in 0..5 {
        let mut max_row = column;
        let mut max_value = a[column][column].abs();
        for row in (column + 1)..5 {
            if a[row][column].abs() > max_value {
                max_value = a[row][column].abs();
                max_row = row;
            }
        }
        a.swap(column, max_row);
        b.swap(column, max_row);
        let pivot = a[column][column];
        for row in (column + 1)..5 {
            let factor = a[row][column] / pivot;
            for item in column..5 {
                a[row][item] -= factor * a[column][item];
            }
            b[row] -= factor * b[column];
        }
    }
    let mut result = [0.0; 5];
    for row in (0..5).rev() {
        let mut value = b[row];
        for column in (row + 1)..5 {
            value -= a[row][column] * result[column];
        }
        result[row] = value / a[row][row];
    }
    result
}

#[allow(clippy::needless_range_loop)]
fn poly_fit_4(times: &[f64], values: &[f64]) -> [f64; 5] {
    let mut matrix = [[0.0; 5]; 5];
    let mut vector = [0.0; 5];
    for (&time, &value) in times.iter().zip(values) {
        let mut ti = 1.0;
        for row in 0..5 {
            let mut tj = 1.0;
            for column in 0..5 {
                matrix[row][column] += ti * tj;
                tj *= time;
            }
            vector[row] += ti * value;
            ti *= time;
        }
    }
    solve_5x5(&mut matrix, &mut vector)
}

fn cam_state_poll(
    conn: &mut BinaryConnection<std::net::TcpStream>,
    state: u8,
) -> Result<u8, ConnError> {
    conn.send(&Command::CamState(CamState { state }), BusAddr::Pi)?;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(ConnError::Timeout);
        }
        let envelope = conn.recv_timeout(remaining)?;
        if envelope.src == BusAddr::Pi
            && let Message::CamState(camera_state) = envelope.message
        {
            return Ok(camera_state.state);
        }
    }
}

fn start_camera(
    conn: &mut BinaryConnection<std::net::TcpStream>,
    config: &CamConfig,
    label: &str,
) -> Result<(), ConnError> {
    let timeout = Duration::from_secs(2);
    conn.send(&Command::CamConfig(config.clone()), BusAddr::Pi)?;
    let _ = seq::recv_msg(conn, timeout)?;
    conn.send(&Command::CamConfigReq(CamConfigReq), BusAddr::Pi)?;
    let _ = seq::recv_msg(conn, timeout)?;
    let state = cam_state_poll(conn, 0x00)?;
    println!("[{label}] stop state=0x{state:02X}");
    for attempt in 1..=8 {
        thread::sleep(Duration::from_secs(5));
        let state = cam_state_poll(conn, 0x03)?;
        println!("[{label}] poll {attempt}: state=0x{state:02X}");
        if state == 0x01 {
            return Ok(());
        }
    }
    Err(ConnError::Timeout)
}

fn unique_ball_points(shot: &ShotCollector) -> Vec<(i16, &ironsight::protocol::shot::PrcPoint)> {
    let mut seen = HashSet::new();
    let mut points = Vec::new();
    for page in &shot.ball_prc_pages {
        for point in &page.points {
            if seen.insert((point.index, point.time, point.buf_idx)) {
                points.push((page.sequence, point));
            }
        }
    }
    points.sort_by_key(|(_, point)| (point.time, point.index));
    points
}

fn unique_club_points(shot: &ShotCollector) -> Vec<&ironsight::protocol::shot::ClubPrcPoint> {
    let mut seen = HashSet::new();
    let mut points = Vec::new();
    for page in &shot.club_prc_pages {
        for point in &page.points {
            if seen.insert((point.index, point.buf_ofs, point.time)) {
                points.push(point);
            }
        }
    }
    points.sort_by_key(|point| (point.buf_ofs, point.time, point.index));
    points
}

fn write_shot_outputs(session_dir: &Path, shot: &ShotCollector) -> AppResult<()> {
    let shot_dir = session_dir.join(format!("shot_{:06}", shot.local_shot_id));
    fs::create_dir_all(&shot_dir)?;
    write_ball_csv(&shot_dir.join("radar_ball_raw.csv"), shot)?;
    write_club_csv(&shot_dir.join("radar_club_raw.csv"), shot)?;
    if let Some(result) = &shot.camera_result {
        write_camera_csv(&shot_dir.join("camera_ball_raw.csv"), shot, result, |id| {
            id == 0
        })?;
        write_camera_csv(&shot_dir.join("camera_club_raw.csv"), shot, result, |id| {
            id == 1
        })?;
        write_camera_csv(
            &shot_dir.join("camera_reference_raw.csv"),
            shot,
            result,
            |id| id >= 2,
        )?;
        write_json(&shot_dir.join("gvp_result.json"), result)?;
        if let Some(raw) = &shot.gvp_result_raw {
            atomic_write(&shot_dir.join("gvp_result_raw.json"), raw.as_bytes())?;
        }
    }
    write_summary(&shot_dir.join("summary.json"), shot)?;
    Ok(())
}

fn write_ball_csv(path: &Path, shot: &ShotCollector) -> io::Result<()> {
    let mut csv = String::from(
        "shot_id,guid,page_sequence,index,buf_idx,flags,time_tick,time_seconds,n,az_deg,el_deg,radial_velocity_mps,dist_m,sync_idx,sync_buf,snr,peak,az1_deg,az2_deg,az3_deg,el1_deg,el2_deg,pk0,pk1,pk2,pk3,pk4,pk5\n",
    );
    for (sequence, point) in unique_ball_points(shot) {
        let values = format!(
            "{},{},{},{},{},{},{},{:.9},{:.9},{:.6},{:.6},{:.6},{:.6},{},{},{},{},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6}\n",
            shot.local_shot_id,
            shot.guid,
            sequence,
            point.index,
            point.buf_idx,
            point.flags,
            point.time,
            f64::from(point.time) * 26.7e-6,
            point.n,
            point.az,
            point.el,
            point.vel,
            point.dist,
            point.sync_idx,
            point.sync_buf,
            point.snr,
            point.peak,
            point.az1,
            point.az2,
            point.az3,
            point.el1,
            point.el2,
            point.pk[0],
            point.pk[1],
            point.pk[2],
            point.pk[3],
            point.pk[4],
            point.pk[5],
        );
        csv.push_str(&values);
    }
    atomic_write(path, csv.as_bytes())
}

fn write_club_csv(path: &Path, shot: &ShotCollector) -> io::Result<()> {
    let mut csv = String::from(
        "shot_id,guid,index,buf_ofs,phase,peak,snr,buf_idx,time_tick,time_seconds,n,az_deg,el_deg,velocity_mps,velocity2_mps,dist_m,f30,f33,version,f39,f42,f45,az1_deg,az2_deg,az3_deg,el1_deg,el2_deg,pk0,pk1,pk2,pk3,pk4,pk5\n",
    );
    for point in unique_club_points(shot) {
        let phase = match point.buf_ofs.cmp(&0) {
            std::cmp::Ordering::Less => "pre_impact",
            std::cmp::Ordering::Equal => "impact",
            std::cmp::Ordering::Greater => "post_impact",
        };
        let values = format!(
            "{},{},{},{},{},{},{},{},{},{:.9},{:.9},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{},{},{},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6}\n",
            shot.local_shot_id,
            shot.guid,
            point.index,
            point.buf_ofs,
            phase,
            point.peak,
            point.snr,
            point.buf_idx,
            point.time,
            f64::from(point.time) * 26.7e-6,
            point.n,
            point.az,
            point.el,
            point.vel,
            point.vel2,
            point.dist,
            point.f30,
            point.f33,
            point.version,
            point.f39,
            point.f42,
            point.f45,
            point.az1,
            point.az2,
            point.az3,
            point.el1,
            point.el2,
            point.pk[0],
            point.pk[1],
            point.pk[2],
            point.pk[3],
            point.pk[4],
            point.pk[5],
        );
        csv.push_str(&values);
    }
    atomic_write(path, csv.as_bytes())
}

fn write_camera_csv(
    path: &Path,
    shot: &ShotCollector,
    result: &BallTrackerResult,
    include: impl Fn(i32) -> bool,
) -> io::Result<()> {
    let mut csv = String::from(
        "shot_id,guid,track_id,point_index,frame_number,timestamp,u_px,v_px,radius_px,circularity_factor,shutter_time_ms\n",
    );
    let mut seen = HashSet::new();
    for track in result.tracks.iter().filter(|track| include(track.track_id)) {
        let length = track_common_length(track);
        for index in 0..length {
            let key = (
                track.track_id,
                track.frame_number[index],
                track.timestamp[index].to_bits(),
            );
            if !seen.insert(key) {
                continue;
            }
            let values = format!(
                "{},{},{},{},{},{:.9},{:.6},{:.6},{:.6},{:.6},{:.6}\n",
                shot.local_shot_id,
                shot.guid,
                track.track_id,
                index,
                track.frame_number[index],
                track.timestamp[index],
                track.u[index],
                track.v[index],
                track.radius[index],
                track.circularity_factor[index],
                track.shutter_time_ms[index],
            );
            csv.push_str(&values);
        }
    }
    atomic_write(path, csv.as_bytes())
}

fn track_common_length(track: &Track) -> usize {
    [
        track.frame_number.len(),
        track.timestamp.len(),
        track.u.len(),
        track.v.len(),
        track.radius.len(),
        track.circularity_factor.len(),
        track.shutter_time_ms.len(),
    ]
    .into_iter()
    .min()
    .unwrap_or(0)
}

fn camera_point_count(
    result: Option<&BallTrackerResult>,
    predicate: impl Fn(i32) -> bool,
) -> usize {
    let Some(result) = result else {
        return 0;
    };
    let mut seen = HashSet::new();
    for track in result
        .tracks
        .iter()
        .filter(|track| predicate(track.track_id))
    {
        for index in 0..track_common_length(track) {
            seen.insert((
                track.track_id,
                track.frame_number[index],
                track.timestamp[index].to_bits(),
            ));
        }
    }
    seen.len()
}

fn validation_warnings(shot: &ShotCollector) -> Vec<String> {
    let mut warnings = shot.warnings.clone();
    if shot.prc_fetch.enabled && !shot.prc_fetch.ball_complete {
        push_unique(&mut warnings, "球 PRC 主动分页未完成。");
    }
    if shot.prc_fetch.enabled && !shot.prc_fetch.club_complete {
        push_unique(&mut warnings, "杆头 PRC 主动分页未完成。");
    }
    if shot.prc_fetch.ball_timed_out {
        push_unique(
            &mut warnings,
            "球 PRC 分页请求超时，已继续完成本杆并 re-arm。",
        );
    }
    if shot.prc_fetch.club_timed_out {
        push_unique(&mut warnings, "杆头 PRC 分页请求超时，已继续 re-arm。");
    }
    if shot.prc_fetch.ball_page_limit_reached {
        push_unique(&mut warnings, "球 PRC 达到 64 页安全上限，采集可能不完整。");
    }
    if shot.prc_fetch.club_page_limit_reached {
        push_unique(
            &mut warnings,
            "杆头 PRC 达到 64 页安全上限，采集可能不完整。",
        );
    }
    let ball_points = unique_ball_points(shot);
    let club_points = unique_club_points(shot);
    if shot.shot_lifecycle_complete && ball_points.is_empty() {
        push_unique(&mut warnings, "未收到球 PRC 雷达点。");
    }
    let expected_ball = shot
        .tracking_status
        .iter()
        .map(|status| usize::from(status.prc_tracking_count))
        .max()
        .unwrap_or(0)
        .max(shot.prc_fetch.ball_expected_points);
    if expected_ball > 0 && ball_points.len() < expected_ball {
        push_unique(
            &mut warnings,
            &format!(
                "球 PRC 点数少于 TrackingStatus：收到 {}，期望至少 {}。",
                ball_points.len(),
                expected_ball
            ),
        );
    }
    let expected_club = shot
        .club
        .as_ref()
        .map_or(0, |club| usize::from(club.num_club_prc_points))
        .max(shot.prc_fetch.club_expected_points);
    if club_points.len() < expected_club {
        push_unique(
            &mut warnings,
            &format!(
                "杆头 PRC 点数少于 ClubResult：收到 {}，期望 {}。",
                club_points.len(),
                expected_club
            ),
        );
    }
    if shot.shot_lifecycle_complete && shot.flight.is_none() && shot.flight_v1.is_none() {
        push_unique(&mut warnings, "未收到 D4 或 E8 飞行结果。");
    }
    if shot.shot_lifecycle_complete && shot.camera_result.is_none() {
        push_unique(&mut warnings, "GVP RESULT 尚未到达；已先保存雷达文件。");
    }
    if shot.camera_result.is_some() && shot.gvp_result_raw.is_none() {
        push_unique(
            &mut warnings,
            "已收到 GVP RESULT，但未捕获到对应原始 JSON。",
        );
    }
    if let Some(result) = &shot.camera_result {
        for track in &result.tracks {
            let lengths = [
                track.frame_number.len(),
                track.timestamp.len(),
                track.u.len(),
                track.v.len(),
                track.radius.len(),
                track.circularity_factor.len(),
                track.shutter_time_ms.len(),
            ];
            if lengths.iter().any(|length| *length != lengths[0]) {
                push_unique(
                    &mut warnings,
                    &format!(
                        "相机 trackId={} 的平行数组长度不一致 {:?}；CSV 只写共同有效部分。",
                        track.track_id, lengths
                    ),
                );
            }
        }
    }
    warnings
}

fn push_unique(warnings: &mut Vec<String>, warning: &str) {
    if !warnings.iter().any(|existing| existing == warning) {
        warnings.push(warning.to_owned());
    }
}

fn write_summary(path: &Path, shot: &ShotCollector) -> AppResult<()> {
    let total_ball: usize = shot
        .ball_prc_pages
        .iter()
        .map(|page| page.points.len())
        .sum();
    let total_club: usize = shot
        .club_prc_pages
        .iter()
        .map(|page| page.points.len())
        .sum();
    let ball_count = unique_ball_points(shot).len();
    let club_count = unique_club_points(shot).len();
    let warnings = validation_warnings(shot);
    let expected_ball = shot
        .tracking_status
        .iter()
        .map(|status| usize::from(status.prc_tracking_count))
        .max()
        .unwrap_or(0)
        .max(shot.prc_fetch.ball_expected_points);
    let expected_club = shot
        .club
        .as_ref()
        .map_or(0, |club| usize::from(club.num_club_prc_points))
        .max(shot.prc_fetch.club_expected_points);
    let speed_profile: Value = shot.speed_profile.as_ref().map_or(Value::Null, |profile| {
        json!({
            "flags": profile.flags,
            "numPre": profile.num_pre,
            "numPost": profile.num_post,
            "scaleFactor": profile.scale_factor,
            "timeIntervalSeconds": profile.time_interval,
            "speedsMps": profile.speeds,
        })
    });
    let flight = shot.flight.as_ref().map_or(Value::Null, flight_json);
    let flight_v1 = shot.flight_v1.as_ref().map_or(Value::Null, flight_v1_json);
    let club = shot.club.as_ref().map_or(Value::Null, club_json);
    let spin = shot.spin.as_ref().map_or(Value::Null, spin_json);
    let summary = json!({
        "schemaVersion": SCHEMA_VERSION,
        "shotId": shot.local_shot_id,
        "guid": shot.guid,
        "triggerEpoch": shot.trigger_epoch,
        "shotLifecycleComplete": shot.shot_lifecycle_complete,
        "radarComplete": shot.radar_complete,
        "activePaginationEnabled": shot.prc_fetch.enabled,
        "pagination": {
            "ballComplete": shot.prc_fetch.ball_complete,
            "clubComplete": shot.prc_fetch.club_complete,
            "ballTimedOut": shot.prc_fetch.ball_timed_out,
            "clubTimedOut": shot.prc_fetch.club_timed_out,
            "ballPageLimitReached": shot.prc_fetch.ball_page_limit_reached,
            "clubPageLimitReached": shot.prc_fetch.club_page_limit_reached,
            "ballPagesRequested": shot.prc_fetch.ball_pages_requested,
            "ballPagesReceived": shot.prc_fetch.ball_pages_received,
            "clubPagesRequested": shot.prc_fetch.club_pages_requested,
            "clubPagesReceived": shot.prc_fetch.club_pages_received,
        },
        "cameraResultReceived": shot.camera_result.is_some(),
        "rawGvpJsonCaptured": shot.gvp_result_raw.is_some(),
        "ballPrcPageCount": shot.ball_prc_pages.len(),
        "clubPrcPageCount": shot.club_prc_pages.len(),
        "ballPrcPointCount": ball_count,
        "clubPrcPointCount": club_count,
        "expectedBallPrcPointCount": expected_ball,
        "expectedClubPrcPointCount": expected_club,
        "duplicateBallPrcPoints": total_ball.saturating_sub(ball_count),
        "duplicateClubPrcPoints": total_club.saturating_sub(club_count),
        "cameraBallPointCount": camera_point_count(shot.camera_result.as_ref(), |id| id == 0),
        "cameraClubPointCount": camera_point_count(shot.camera_result.as_ref(), |id| id == 1),
        "cameraReferencePointCount": camera_point_count(shot.camera_result.as_ref(), |id| id >= 2),
        "flight": flight,
        "flightV1": flight_v1,
        "club": club,
        "spin": spin,
        "speedProfile": speed_profile,
        "warnings": warnings,
    });
    write_json(path, &summary)
}

fn flight_json(value: &FlightResult) -> Value {
    json!({
        "shotCounter": value.total,
        "trackTimeSeconds": value.track_time,
        "startPositionM": value.start_position,
        "launchSpeedMps": value.launch_speed,
        "launchAzimuthDeg": value.launch_azimuth,
        "launchElevationDeg": value.launch_elevation,
        "carryDistanceM": value.carry_distance,
        "flightTimeSeconds": value.flight_time,
        "maximumHeightM": value.max_height,
        "landingPositionM": value.landing_position,
        "landingVelocityMps": value.landing_velocity,
        "backspinRpm": value.backspin_rpm,
        "sidespinRpm": value.sidespin_rpm,
        "riflespinRpm": value.riflespin_rpm,
        "landingSpinRpm": value.landing_spin_rpm,
        "clubheadSpeedMps": value.clubhead_speed,
        "clubStrikeDirectionDeg": value.club_strike_direction,
        "clubAttackAngleDeg": value.club_attack_angle,
        "clubheadSpeedPostMps": value.clubhead_speed_post,
        "clubSwingPlaneTiltDeg": value.club_swing_plane_tilt,
        "clubSwingPlaneRotationDeg": value.club_swing_plane_rotation,
        "clubEffectiveLoftDeg": value.club_effective_loft,
        "clubFaceAngleDeg": value.club_face_angle,
        "polyScale": value.poly_scale,
        "polyX": value.poly_x,
        "polyY": value.poly_y,
        "polyZ": value.poly_z,
    })
}

fn flight_v1_json(value: &FlightResultV1) -> Value {
    json!({
        "shotCounter": value.total,
        "clubVelocityMps": value.club_velocity,
        "ballVelocityMps": value.ball_velocity,
        "flightTimeSeconds": value.flight_time,
        "distanceM": value.distance,
        "heightM": value.height,
        "lateralM": value.lateral,
        "elevationDeg": value.elevation,
        "azimuthDeg": value.azimuth,
        "trackedTimeSeconds": value.tracked_time,
        "drag": value.drag,
        "backspinRpm": value.backspin_rpm,
        "sidespinRpm": value.sidespin_rpm,
        "acceleration": value.acceleration,
        "clubStrikeDirectionDeg": value.club_strike_direction,
        "polyScale": value.poly_scale,
        "polyX": value.poly_x,
        "polyY": value.poly_y,
        "polyZ": value.poly_z,
    })
}

fn club_json(value: &ClubResult) -> Value {
    json!({
        "numClubPrcPoints": value.num_club_prc_points,
        "flags": value.flags,
        "preClubSpeedMps": value.pre_club_speed,
        "postClubSpeedMps": value.post_club_speed,
        "strikeDirectionDeg": value.strike_direction,
        "attackAngleDeg": value.attack_angle,
        "faceAngleDeg": value.face_angle,
        "dynamicLoftDeg": value.dynamic_loft,
        "smashFactor": value.smash_factor,
        "dispersionCorrection": value.dispersion_correction,
        "swingPlaneHorizontalDeg": value.swing_plane_horizontal,
        "swingPlaneVerticalDeg": value.swing_plane_vertical,
        "clubAzimuthDeg": value.club_azimuth,
        "clubElevationDeg": value.club_elevation,
        "clubOffsetM": value.club_offset,
        "clubHeightM": value.club_height,
        "polyScale": value.poly_scale,
        "polyCoeffs": value.poly_coeffs,
        "preImpactTimeMs": value.pre_impact_time,
        "postImpactTimeMs": value.post_impact_time,
        "clubToBallTimeMs": value.club_to_ball_time,
    })
}

fn spin_json(value: &SpinResult) -> Value {
    let antenna_data: Vec<Value> = value
        .antenna_data
        .iter()
        .map(|group| {
            Value::Array(
                group
                    .iter()
                    .map(|item| {
                        json!({
                            "spinRpm": item.spin_rpm,
                            "peak": item.peak,
                            "snr": item.snr,
                        })
                    })
                    .collect(),
            )
        })
        .collect();
    json!({
        "version": value.version,
        "antennaData": antenna_data,
        "pmSpinRawRpm": value.pm_spin_raw,
        "pmSpinFinalRpm": value.pm_spin_final,
        "pmSpinConfidence": value.pm_spin_confidence,
        "liftSpinRpm": value.lift_spin,
        "spinValidateExpectedRpm": value.spin_validate_expected,
        "spinValidateLowRpm": value.spin_validate_low,
        "spinValidateHighRpm": value.spin_validate_high,
        "spinValidateScaling": value.spin_validate_scaling,
        "spinMethod": value.spin_method,
        "spinFlags": value.spin_flags,
        "launchSpinRpm": value.launch_spin,
        "amSpinRpm": value.am_spin,
        "pmSpinRpm": value.pm_spin,
        "spinAxisDeg": value.spin_axis,
        "aodSpinRpm": value.aod_spin,
        "pllSpinRpm": value.pll_spin,
    })
}

fn write_device_json(
    session_dir: &Path,
    dsp: &seq::DspSync,
    avr: &seq::AvrSync,
    pi: &seq::PiSync,
) -> AppResult<()> {
    let value = json!({
        "schemaVersion": SCHEMA_VERSION,
        "model": dsp.hw_info.device_gen().label(),
        "dspType": dsp.hw_info.dsp_type,
        "pcbRevision": dsp.hw_info.pcb,
        "ssid": pi.ssid,
        "dspFirmware": dsp.dev_info.text,
        "avrFirmware": avr.dev_info.text,
        "piFirmware": pi.dev_info.text,
        "productInfo": dsp.prod_info.iter().map(|item| item.text.as_str()).collect::<Vec<_>>(),
    });
    write_json(&session_dir.join("device.json"), &value)
}

fn write_session_from_collectors(
    session: &SessionInfo,
    options: &Options,
    collectors: &HashMap<String, ShotCollector>,
) -> AppResult<()> {
    let complete = collectors
        .values()
        .filter(|shot| shot.shot_lifecycle_complete)
        .count();
    let camera = collectors
        .values()
        .filter(|shot| shot.camera_result.is_some())
        .count();
    write_session_json(session, options, collectors.len(), complete, camera)
}

fn write_session_json(
    session: &SessionInfo,
    options: &Options,
    shot_count: usize,
    completed_shots: usize,
    camera_results: usize,
) -> AppResult<()> {
    let value = json!({
        "schemaVersion": SCHEMA_VERSION,
        "exporterVersion": env!("CARGO_PKG_VERSION"),
        "startedEpoch": session.started_epoch,
        "updatedEpoch": epoch_now(),
        "mode": options.mode_name,
        "modeCode": options.mode,
        "radarCal": {
            "rangeMm": options.range_mm,
            "heightMm": options.height_mm,
        },
        "binaryAddress": options.device.to_string(),
        "gvpAddress": format!("{}:{DEFAULT_PORT}", options.device.ip()),
        "outputDirectory": session.session_dir.display().to_string(),
        "activePaginationEnabled": true,
        "shotCount": shot_count,
        "completedShotCount": completed_shots,
        "cameraResultCount": camera_results,
    });
    write_json(&session.session_dir.join("session.json"), &value)
}

fn write_json(path: &Path, value: &impl serde::Serialize) -> AppResult<()> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    atomic_write(path, &bytes)?;
    Ok(())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("output");
    let temporary = path.with_file_name(format!("{file_name}.tmp"));
    let mut file = File::create(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    match fs::rename(&temporary, path) {
        Ok(()) => Ok(()),
        Err(_) if path.exists() => {
            fs::remove_file(path)?;
            fs::rename(temporary, path)
        }
        Err(error) => Err(error),
    }
}

fn touch(path: &Path) -> io::Result<()> {
    OpenOptions::new().create(true).append(true).open(path)?;
    Ok(())
}

fn log_event(session_dir: &Path, message: &str) {
    let result = OpenOptions::new()
        .create(true)
        .append(true)
        .open(session_dir.join("session.log"))
        .and_then(|mut file| writeln!(file, "{:.6} {message}", epoch_now()));
    if let Err(error) = result {
        eprintln!("warning: could not write session.log: {error}");
    }
}

fn create_session_directory(parent: &Path, epoch: f64) -> io::Result<PathBuf> {
    fs::create_dir_all(parent)?;
    let stamp = utc_stamp(epoch as u64);
    for suffix in 0..1000u16 {
        let name = if suffix == 0 {
            stamp.clone()
        } else {
            format!("{stamp}_{suffix:03}")
        };
        let path = parent.join(name);
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not create a unique session directory",
    ))
}

fn epoch_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn new_guid(counter: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let seconds = now.as_secs();
    let nanos = now.subsec_nanos();
    let pid = process::id();
    format!(
        "{{{:08x}-{:04x}-4{:03x}-a{:03x}-{:08x}{:04x}}}",
        seconds as u32,
        (seconds >> 32) as u16,
        (nanos >> 20) & 0x0fff,
        pid & 0x0fff,
        nanos,
        counter & 0xffff,
    )
}

fn utc_stamp(epoch_seconds: u64) -> String {
    let days = (epoch_seconds / 86_400) as i64;
    let seconds = epoch_seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = seconds / 3600;
    let minute = (seconds % 3600) / 60;
    let second = seconds % 60;
    format!("{year:04}-{month:02}-{day:02}_{hour:02}{minute:02}{second:02}Z")
}

fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }
    (year, month, day)
}
