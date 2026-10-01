#!/usr/bin/env python3
"""Frozen-model, default-atmosphere comparison; never fits coefficients or shot offsets.

Run from any directory: python3 /path/to/evaluate_mevo_models.py
Requires g++, numpy, pandas and matplotlib. Outputs stay beside this script.
"""
from pathlib import Path
import hashlib
import io
import json
import platform
import subprocess
import tempfile

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np
import pandas as pd

HERE = Path(__file__).resolve().parent
SESSION = HERE.parent
ROOT = next(p for p in HERE.parents if (p / "offline_model/data_and_model/model/libgolf").is_dir())
DATA = ROOT / "offline_model/data_and_model"
LIB = DATA / "model/libgolf"
SOURCE = DATA / "validation_5_environment/optimization_workspace/libgolf_environment_runner.cpp"
COEF = DATA / "validation_6_filtered_dataset/global_optimization_outputs/filtered_665_11_parameter.json"
OUT = HERE / "outputs"
FIG = HERE / "figures"
MODELS = ["original", "optimized", "mevo_d4"]
LABELS = {"original": "Libgolf original", "optimized": "Optimized (frozen 11p)", "mevo_d4": "Mevo D4 fit (not raw truth)"}
COLORS = {"original": "#2878b5", "optimized": "#e57b25", "mevo_d4": "#33945c"}
OBS = ["vr_mps", "az_deg", "el_deg"]
UNITS = ["Radial velocity (m/s)", "Azimuth (deg)", "Elevation (deg)"]
SHOTS = list(range(2, 8))  # User-requested exclusion after inspecting Shot 1 Carry APE.


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def table(frame):
    # No optional tabulate dependency.
    def fmt(x):
        if isinstance(x, (float, np.floating)):
            return f"{x:.3f}"
        return str(x)
    return "\n".join(["| " + " | ".join(map(str, frame.columns)) + " |",
                      "| " + " | ".join(["---"] * len(frame.columns)) + " |"] +
                     ["| " + " | ".join(fmt(x) for x in row) + " |" for row in frame.itertuples(index=False, name=None)])


def project(p, v, origin):
    d = p - origin
    r = np.linalg.norm(d, axis=1)
    return np.column_stack([(d * v).sum(axis=1) / r,
                            np.degrees(np.arctan2(d[:, 2], d[:, 0])),
                            np.degrees(np.arctan2(d[:, 1], np.hypot(d[:, 0], d[:, 2])))])


def d4_state(flight, times):
    coefs = [flight[f"poly{a}"] for a in "XYZ"]
    p = np.column_stack([np.polynomial.polynomial.polyval(times, c) for c in coefs])
    v = np.column_stack([np.polynomial.polynomial.polyval(times, np.arange(1, len(c)) * c[1:]) for c in coefs])
    return p, v


def predict(trajectory, times):
    assert times.min() >= trajectory.t_s.min() and times.max() <= trajectory.t_s.max()
    arr = np.column_stack([np.interp(times, trajectory.t_s, trajectory[col])
                           for col in ["X_m", "Y_m", "Z_m", "VX_mps", "VY_mps", "VZ_mps"]])
    return arr[:, :3], arr[:, 3:]


def metrics(frame, grouping):
    rows = []
    for keys, group in frame.groupby(grouping):
        if not isinstance(keys, tuple):
            keys = (keys,)
        row = dict(zip(grouping, keys))
        row["n_points"] = len(group)
        row["n_shots"] = group.shot_id.nunique()
        for name in OBS:
            e = group[f"error_{name}"].to_numpy()
            row[f"rmse_{name}"] = float(np.sqrt(np.mean(e * e)))
            row[f"mae_{name}"] = float(np.mean(np.abs(e)))
            row[f"bias_{name}"] = float(np.mean(e))
        rows.append(row)
    return pd.DataFrame(rows)


def savefig(fig, name):
    fig.savefig(FIG / name, dpi=165, bbox_inches="tight", facecolor="white")
    plt.close(fig)


def main():
    OUT.mkdir(exist_ok=True)
    FIG.mkdir(exist_ok=True)
    plt.rcParams.update({"font.size": 10, "axes.spines.top": False, "axes.spines.right": False})
    session = json.loads((SESSION / "session.json").read_text())
    distance = session["radarCal"]["rangeMm"] / 1000
    assert distance > 0 and session["shotCount"] == 7
    flights, raw, origins, inputs, quality = {}, {}, {}, [], []
    source_files = [SESSION / "session.json", COEF, SOURCE, HERE / "trajectory_main.cpp", Path(__file__)]
    for sid in SHOTS:
        folder = SESSION / f"shot_{sid:06d}"
        summary = json.loads((folder / "summary.json").read_text())
        f = summary["flight"]
        radar = pd.read_csv(folder / "radar_ball_raw.csv")
        assert radar.shot_id.eq(sid).all() and radar.guid.nunique() == 1
        assert len(radar) == summary["ballPrcPointCount"]
        assert radar.n.iloc[0] == 0 and (np.diff(radar.n) > 0).all()
        assert not radar["index"].duplicated().any()
        assert np.isfinite(radar[["n", "az_deg", "el_deg", "radial_velocity_mps"]]).all().all()
        assert radar.dist_m.eq(0).all()
        radar["point_id"] = np.arange(len(radar))
        az, el = np.radians(radar.loc[0, ["az_deg", "el_deg"]].to_numpy(dtype=float))
        u = np.array([np.cos(el) * np.cos(az), np.sin(el), np.cos(el) * np.sin(az)])
        origins[sid] = np.array(f["startPositionM"]) - distance * u
        row = dict(shot_id=sid, speed_mps=f["launchSpeedMps"], speed_mph=f["launchSpeedMps"] / 0.44704,
                   elevation_deg=f["launchElevationDeg"], azimuth_deg=f["launchAzimuthDeg"],
                   backspin_rpm=f["backspinRpm"], mevo_sidespin_rpm=f["sidespinRpm"],
                   libgolf_sidespin_rpm=-f["sidespinRpm"],
                   spin_rpm=np.hypot(f["backspinRpm"], f["sidespinRpm"]),
                   impact_time_s=0, radar_time_offset_s=0,
                   temp_f=59, humidity_pct=0, pressure_inhg=29.92, elevation_ft=0, wind_mph=0,
                   reference_carry_m=f["carryDistanceM"], reference_peak_m=f["maximumHeightM"],
                   reference_flight_s=f["flightTimeSeconds"], track_s=radar.n.max(), n_points=len(radar))
        for axis, val, origin in zip("XYZ", f["startPositionM"], origins[sid]):
            row[f"start_{axis}_m"] = val
            row[f"radar_{axis}_m"] = origin
        inputs.append(row)
        quality.append(dict(shot_id=sid, n_points=len(radar), track_s=radar.n.max(),
                            zero_distance_points=int(radar.dist_m.eq(0).sum()),
                            counter_wraps=int((np.diff(radar.time_tick) < 0).sum()),
                            max_time_discrepancy_s=float(abs(radar.n - radar["index"] / 37500).max()),
                            snr_lt20=int(radar.snr.lt(20).sum()),
                            camera_points=len(pd.read_csv(folder / "camera_ball_raw.csv"))))
        flights[sid], raw[sid] = f, radar
        source_files.extend([folder / "summary.json", folder / "radar_ball_raw.csv", folder / "camera_ball_raw.csv"])
    initial = pd.DataFrame(inputs)
    initial.to_csv(OUT / "initial_conditions.csv", index=False)
    pd.DataFrame(quality).to_csv(OUT / "data_quality.csv", index=False)
    assert initial.n_points.sum() == 1233

    coefficients = json.loads(COEF.read_text())["vector"]
    assert len(coefficients) == 11
    q24 = np.zeros(24)
    q24[[0, 1, 2, 6, 7, 8, 12, 13, 14, 18, 20]] = coefficients
    (OUT / "calibration.csv").write_text("name,value\nmode,Q24\n" + "".join(f"p{i},{v:.17g}\n" for i, v in enumerate(q24)))
    with tempfile.TemporaryDirectory(prefix="mevo_libgolf_eval_") as tmp:
        tmp = Path(tmp)
        prefix = SOURCE.read_text().split("\nint main(")[0]
        assert "class BivariateQuadraticModel" in prefix and "} // namespace" in prefix
        (tmp / "runner.cpp").write_text(prefix + "\n" + (HERE / "trajectory_main.cpp").read_text())
        cmd = ["g++", "-std=c++20", "-O3", "-DNDEBUG", f"-I{LIB / 'include'}", str(tmp / "runner.cpp")]
        cmd += [str(LIB / "src" / f) for f in ["math_utils.cpp", "ShotPhysicsContext.cpp", "FlightPhase.cpp", "FlightSimulator.cpp", "ground_physics.cpp"]]
        cmd += ["-o", str(tmp / "runner")]
        subprocess.run(cmd, check=True, capture_output=True, text=True)
        all_trajectories = {}
        for dt in [0.01, 0.001]:
            run = subprocess.run([str(tmp / "runner"), str(OUT / "calibration.csv"), str(dt)],
                                 input=initial.to_csv(index=False), capture_output=True, text=True, check=True)
            tr = pd.read_csv(io.StringIO(run.stdout))
            assert tr.groupby(["shot_id", "model"]).ngroups == 2 * len(SHOTS)
            assert np.isfinite(tr.select_dtypes(include="number")).all().all()
            for _, group in tr.groupby(["shot_id", "model"]):
                assert group.t_s.iloc[0] == 0 and (np.diff(group.t_s) > 0).all()
                assert group.Y_m.iloc[-1] == 0
            all_trajectories[dt] = tr
            tr.to_csv(OUT / ("trajectories.csv" if dt == 0.01 else "trajectories_dt_0p001.csv"), index=False)
    traj = all_trajectories[0.01]
    # Both models must have exactly the same launch state for each shot.
    starts = traj.groupby(["shot_id", "model"]).first()
    for sid in SHOTS:
        np.testing.assert_array_equal(starts.loc[(sid, "original")], starts.loc[(sid, "optimized")])

    comparisons = []
    for sid in SHOTS:
        radar = raw[sid]
        observed = radar[["radial_velocity_mps", "az_deg", "el_deg"]].to_numpy()
        for model in MODELS:
            for offset in [0.0, 0.01, 0.02]:
                times = radar.n.to_numpy() + offset
                if model == "mevo_d4":
                    p, v = d4_state(flights[sid], times)
                else:
                    p, v = predict(traj[(traj.shot_id == sid) & (traj.model == model)], times)
                projected = project(p, v, origins[sid])
                errors = projected - observed
                errors[:, 1:] = (errors[:, 1:] + 180) % 360 - 180
                frame = radar[["shot_id", "point_id", "n", "snr", "index", "time_tick"]].copy()
                frame["model"] = model
                frame["time_offset_s"] = offset
                frame["model_time_s"] = times
                frame["evaluation_mask"] = frame.point_id > 0
                for j, name in enumerate(OBS):
                    frame[f"observed_{name}"] = observed[:, j]
                    frame[f"predicted_{name}"] = projected[:, j]
                    frame[f"error_{name}"] = errors[:, j]
                comparisons.append(frame)
    comparison = pd.concat(comparisons, ignore_index=True)
    main_points = comparison[comparison.time_offset_s.eq(0)]
    main_points.to_csv(OUT / "radar_comparison.csv", index=False)
    evaluated = comparison[comparison.evaluation_mask]
    main_eval = evaluated[evaluated.time_offset_s.eq(0)]
    per_shot = metrics(main_eval, ["model", "shot_id"])
    overall = metrics(main_eval, ["model"])
    balanced = per_shot.groupby("model")[[f"rmse_{v}" for v in OBS]].mean().add_prefix("mean_shot_").reset_index()
    overall = overall.merge(balanced, on="model")
    sensitivity = metrics(evaluated, ["model", "time_offset_s"])
    snr_metrics = metrics(main_eval[main_eval.snr >= 20], ["model"])
    without_shot1 = metrics(main_eval[main_eval.shot_id != 1], ["model"])
    for frame, name in [(per_shot, "radar_metrics_by_shot"), (overall, "overall_metrics"),
                        (sensitivity, "time_offset_sensitivity"), (snr_metrics, "snr20_sensitivity"),
                        (without_shot1, "shots_2_to_7_sensitivity")]:
        frame.to_csv(OUT / f"{name}.csv", index=False)

    endpoints, convergence, fine_comparisons = [], [], []
    for (sid, model), group in traj.groupby(["shot_id", "model"]):
        fine = all_trajectories[0.001]
        fine = fine[(fine.shot_id == sid) & (fine.model == model)]
        last, last_fine = group.iloc[-1], fine.iloc[-1]
        values = {"carry_m": np.hypot(last.X_m, last.Z_m), "peak_m": group.Y_m.max(), "flight_s": last.t_s}
        reference = {"carry_m": flights[sid]["carryDistanceM"], "peak_m": flights[sid]["maximumHeightM"], "flight_s": flights[sid]["flightTimeSeconds"]}
        row = {"shot_id": sid, "model": model}
        for key in values:
            row[f"predicted_{key}"] = values[key]
            row[f"mevo_{key}"] = reference[key]
            row[f"error_{key}"] = values[key] - reference[key]
        row["carry_ape_pct"] = 100 * abs(row["error_carry_m"]) / reference["carry_m"]
        endpoints.append(row)
        coarse_obs = project(*predict(group, raw[sid].n.to_numpy()), origins[sid])
        fine_obs = project(*predict(fine, raw[sid].n.to_numpy()), origins[sid])
        fine_errors = fine_obs - raw[sid][["radial_velocity_mps", "az_deg", "el_deg"]].to_numpy()
        fine_errors[:, 1:] = (fine_errors[:, 1:] + 180) % 360 - 180
        fine_frame = pd.DataFrame({"shot_id": sid, "model": model, "point_id": np.arange(len(fine_errors))})
        for j, name in enumerate(OBS):
            fine_frame[f"error_{name}"] = fine_errors[:, j]
        fine_comparisons.append(fine_frame[fine_frame.point_id > 0])
        convergence.append(dict(shot_id=sid, model=model,
                                carry_delta_m=values["carry_m"] - np.hypot(last_fine.X_m, last_fine.Z_m),
                                peak_delta_m=values["peak_m"] - fine.Y_m.max(),
                                flight_delta_s=values["flight_s"] - last_fine.t_s,
                                max_vr_delta_mps=abs(coarse_obs[:, 0] - fine_obs[:, 0]).max(),
                                max_az_delta_deg=abs(coarse_obs[:, 1] - fine_obs[:, 1]).max(),
                                max_el_delta_deg=abs(coarse_obs[:, 2] - fine_obs[:, 2]).max()))
    endpoint = pd.DataFrame(endpoints)
    convergence = pd.DataFrame(convergence)
    endpoint.to_csv(OUT / "mevo_summary_comparison.csv", index=False)
    convergence.to_csv(OUT / "numerical_convergence.csv", index=False)
    fine_metrics = metrics(pd.concat(fine_comparisons, ignore_index=True), ["model"])
    fine_metrics.to_csv(OUT / "radar_metrics_dt_0p001.csv", index=False)
    endpoint_metrics = []
    for model, group in endpoint.groupby("model"):
        row = {"model": model, "n_shots": len(group), "carry_mape_pct": group.carry_ape_pct.mean()}
        for key in ["carry_m", "peak_m", "flight_s"]:
            e = group[f"error_{key}"]
            row[f"rmse_{key}"] = np.sqrt(np.mean(e * e))
            row[f"mae_{key}"] = abs(e).mean()
            row[f"bias_{key}"] = e.mean()
        endpoint_metrics.append(row)
    endpoint_metrics = pd.DataFrame(endpoint_metrics)
    endpoint_metrics.to_csv(OUT / "mevo_summary_metrics.csv", index=False)

    # Figures: raw observations black; fixed colors for models in time-series.
    for cols, filename in [([0], "radial_velocity.png"), ([1, 2], "radar_angles.png")]:
        if len(cols) == 1:
            fig, axes = plt.subplots(3, 2, figsize=(13, 10), squeeze=False)
            axes = axes.ravel()
        else:
            fig, axes = plt.subplots(len(SHOTS), 2, figsize=(13, 18), squeeze=False)
        for i, sid in enumerate(SHOTS):
            for j, col in enumerate(cols):
                ax = axes[i] if len(cols) == 1 else axes[i, j]
                obsname = OBS[col]
                for model in MODELS:
                    rows = main_points[(main_points.shot_id == sid) & (main_points.model == model)]
                    ax.plot(rows.n, rows[f"predicted_{obsname}"], color=COLORS[model],
                            label=LABELS[model], ls="--" if model == "mevo_d4" else "-", lw=1.6)
                ax.scatter(rows.n, rows[f"observed_{obsname}"], s=6, c="#222222", alpha=.55, label="Mevo raw radar", zorder=4)
                ax.set(title=f"Shot {sid:02d}", xlabel="Time from shared zero (s)", ylabel=UNITS[col])
                ax.grid(alpha=.2)
        handles, labels = ax.get_legend_handles_labels()
        fig.legend(handles, labels, loc="upper center", ncol=2, frameon=False)
        fig.tight_layout(rect=(0, 0, 1, .965 if len(cols) == 1 else .98))
        savefig(fig, filename)

    fig, axes = plt.subplots(1, 3, figsize=(15, 4.3))
    for ax, name, unit in zip(axes, OBS, UNITS):
        for i, model in enumerate(MODELS):
            rows = per_shot[per_shot.model == model].set_index("shot_id").loc[SHOTS]
            ax.bar(np.arange(len(SHOTS)) + (i - 1) * .25, rows[f"rmse_{name}"], .24, color=COLORS[model], label=LABELS[model])
        ax.set(xticks=np.arange(len(SHOTS)), xticklabels=SHOTS, xlabel="Shot", ylabel=f"RMSE: {unit}")
        ax.grid(axis="y", alpha=.2)
    fig.legend(*axes[0].get_legend_handles_labels(), loc="upper center", ncol=3, frameon=False)
    fig.tight_layout(rect=(0, 0, 1, .92))
    savefig(fig, "radar_rmse.png")

    fig, axes = plt.subplots(3, 2, figsize=(13, 10))
    for ax, sid in zip(axes.ravel(), SHOTS):
        for model in MODELS[:2]:
            rows = traj[(traj.shot_id == sid) & (traj.model == model)]
            ax.plot(rows.X_m, rows.Y_m, color=COLORS[model], label=LABELS[model])
        times = np.linspace(0, flights[sid]["flightTimeSeconds"], 501)
        p, _ = d4_state(flights[sid], times)
        ax.plot(p[:, 0], p[:, 1], "--", color=COLORS["mevo_d4"], label="Mevo D4 full fit/extrapolation")
        early = times <= raw[sid].n.max()
        ax.plot(p[early, 0], p[early, 1], color=COLORS["mevo_d4"], lw=3, label="D4 during radar window (also fit)")
        ax.set(title=f"Shot {sid:02d}", xlabel="Downrange (m)", ylabel="Height (m)", ylim=(0, None))
        ax.grid(alpha=.2)
    fig.legend(*ax.get_legend_handles_labels(), loc="upper center", ncol=2, frameon=False)
    fig.tight_layout(rect=(0, 0, 1, .965))
    savefig(fig, "full_trajectories.png")

    # Each shot CSV has a distinct color in summary scatter plots.
    shot_colors = plt.get_cmap("tab10")(np.array(SHOTS) - 1)
    fig, axes = plt.subplots(1, 3, figsize=(15, 4.8))
    for ax, model in zip(axes[:2], MODELS[:2]):
        group = endpoint[endpoint.model == model].set_index("shot_id").loc[SHOTS]
        ax.plot([0, 110], [0, 110], "--", color="gray", lw=1)
        for i, sid in enumerate(SHOTS):
            r = group.loc[sid]
            ax.scatter(r.mevo_carry_m, r.predicted_carry_m, color=shot_colors[i], s=65, label=f"Shot {sid:02d}")
            ax.annotate(str(sid), (r.mevo_carry_m, r.predicted_carry_m), xytext=(4, 4), textcoords="offset points", fontsize=8)
        ax.set(title=LABELS[model], xlabel="Mevo D4 Carry reference (m)", ylabel="Predicted Carry (m)", xlim=(0, 110), ylim=(0, 110))
        ax.set_aspect("equal")
        ax.grid(alpha=.2)
    for i, model in enumerate(MODELS[:2]):
        group = endpoint[endpoint.model == model].set_index("shot_id").loc[SHOTS]
        axes[2].bar(np.arange(len(SHOTS)) + (i - .5) * .36, group.error_carry_m, width=.35, color=COLORS[model], label=LABELS[model])
    axes[2].axhline(0, c="gray", lw=1)
    axes[2].set(xticks=np.arange(len(SHOTS)), xticklabels=SHOTS, xlabel="Shot", ylabel="Predicted - Mevo D4 Carry (m)")
    axes[2].legend(fontsize=8)
    fig.legend(*axes[0].get_legend_handles_labels(), loc="upper center", ncol=len(SHOTS), frameon=False)
    fig.tight_layout(rect=(0, 0, 1, .92))
    savefig(fig, "carry_comparison.png")

    fig, axes = plt.subplots(1, 3, figsize=(14, 4.1))
    for ax, name, unit in zip(axes, OBS, UNITS):
        for model in MODELS:
            group = sensitivity[sensitivity.model == model].sort_values("time_offset_s")
            ax.plot(group.time_offset_s * 1000, group[f"rmse_{name}"], "o-", color=COLORS[model], label=LABELS[model])
        ax.set(xlabel="Common positive time offset (ms)", ylabel=f"Pooled RMSE: {unit}", xticks=[0, 10, 20])
        ax.grid(alpha=.2)
    fig.legend(*axes[0].get_legend_handles_labels(), loc="upper center", ncol=3, frameon=False)
    fig.tight_layout(rect=(0, 0, 1, .9))
    savefig(fig, "time_offset_sensitivity.png")

    source_files += sorted((LIB / "include").rglob("*.hpp")) + sorted((LIB / "src").rglob("*.cpp"))
    manifest = {
        "session": str(SESSION.relative_to(ROOT)), "models": MODELS[:2], "coefficients_fitted": False,
        "environment": {"temp_f": 59, "humidity_pct": 0, "pressure_inhg": 29.92, "elevation_ft": 0, "wind_mph": 0},
        "time": {"impact_time_s": 0, "radar_time_offset_s": 0, "sensitivity_offsets_s": [0, .01, .02],
                 "basis": "n, relative to first ball radar point, assumed impact", "per_shot_fitting": False},
        "geometry": {"D4_axes": ["forward", "up", "lateral"], "libgolf_axes": ["lateral", "forward", "up"],
                     "radar_rotation": "identity assumed", "initial_slant_range_m": distance,
                     "radar_origin": "D4 startPositionM - initial_slant_range_m * first raw LOS unit vector",
                     "sidespin": "libgolf sidespin = -Mevo D4 sidespin", "extrinsics_independently_measured": False},
        "dt_s": .01, "convergence_dt_s": .001, "n_shots": len(SHOTS), "n_raw_points": 1233,
        "included_shot_ids": SHOTS, "excluded_shot_ids": [1],
        "exclusion_reason": "User-requested exclusion after observing Shot 1 optimized Carry APE 10.58%; post-hoc subset, not evidence of invalid measurement",
        "n_evaluated_points_per_model": 1227, "exclusion": "Shot 1 excluded from both models; first point per retained shot used for LOS anchoring; no other point removal",
        "coefficient_q24_indices": [0, 1, 2, 6, 7, 8, 12, 13, 14, 18, 20],
        "python": platform.python_version(), "numpy": np.__version__, "pandas": pd.__version__, "matplotlib": matplotlib.__version__,
        "compiler": subprocess.run(["g++", "--version"], capture_output=True, text=True, check=True).stdout.splitlines()[0],
        "sha256": {str(p.relative_to(ROOT)): sha(p) for p in source_files}}
    (OUT / "run_manifest.json").write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n")
    write_report(initial, overall, per_shot, endpoint, endpoint_metrics, snr_metrics, without_shot1, sensitivity, convergence, fine_metrics)
    print(overall.to_string(index=False))
    print(endpoint_metrics.to_string(index=False))
    print("REPORT:", HERE / "mevo_libgolf_default_evaluation_report.md")


def d4_formula_section(initial):
    """Render decoded final-D4 coefficients, in ascending powers of seconds."""
    sections = []
    for sid in SHOTS:
        flight = json.loads((SESSION / f"shot_{sid:06d}" / "summary.json").read_text())["flight"]
        equations = []
        for axis in "XYZ":
            terms = []
            for power, coefficient in enumerate(flight[f"poly{axis}"]):
                if coefficient == 0:
                    continue
                magnitude = f"{abs(coefficient):.9f}"
                factor = "" if power == 0 else ("t" if power == 1 else f"t^{{{power}}}")
                sign = ("-" if coefficient < 0 else "") if not terms else (" - " if coefficient < 0 else " + ")
                terms.append(sign + magnitude + factor)
            equations.append(f"{axis}(t) &= " + ("".join(terms) or "0"))
        track_time = initial.loc[initial.shot_id.eq(sid), "track_s"].iloc[0]
        sections.append(
            f"#### Shot {sid:02d}\n\n"
            f"绘制区间：`0 ≤ t ≤ {flight['flightTimeSeconds']:.3f} s`；原始雷达窗口：`0 ≤ n ≤ {track_time:.5f} s`。\n\n"
            + "$$\n\\begin{aligned}\n" + " \\\\\n".join(equations) + "\n\\end{aligned}\n$$")
    return "\n\n".join(sections)


def write_report(initial, overall, per_shot, endpoint, endpoint_metrics, snr_metrics, without_shot1, sensitivity, convergence, fine_metrics):
    names = {"original": "Libgolf 原始", "optimized": "11 参数优化", "mevo_d4": "Mevo D4 内部一致性参照"}
    def named(frame):
        frame = frame.copy()
        frame["_order"] = frame.model.map({name: i for i, name in enumerate(MODELS)})
        sort_columns = [c for c in ["敏感性口径", "Shot", "_order"] if c in frame]
        frame = frame.sort_values(sort_columns, kind="stable").drop(columns="_order")
        return frame.replace({"model": names}).rename(columns={"model": "模型"})
    radar_columns = {"rmse_vr_mps": "径向速度 RMSE (m/s)", "rmse_az_deg": "方位角 RMSE (°)", "rmse_el_deg": "仰角 RMSE (°)"}
    input_table = initial[["shot_id", "speed_mps", "elevation_deg", "azimuth_deg", "spin_rpm", "reference_carry_m", "track_s", "n_points"]].rename(columns={
        "shot_id": "Shot", "speed_mps": "球速 (m/s)", "elevation_deg": "发射仰角 (°)", "azimuth_deg": "发射方向 (°)",
        "spin_rpm": "总转速 (rpm)", "reference_carry_m": "D4 Carry (m)", "track_s": "雷达窗口 (s)", "n_points": "雷达点数"})
    rtable = named(overall[["model"] + list(radar_columns)].rename(columns=radar_columns))
    balance = named(overall[["model"] + ["mean_shot_rmse_" + v for v in OBS]].rename(columns={"mean_shot_rmse_" + v: label for v, label in zip(OBS, radar_columns.values())}))
    shot_r = per_shot[per_shot.model != "mevo_d4"][["shot_id", "model"] + list(radar_columns)].rename(columns={"shot_id": "Shot", **radar_columns})
    etable = named(endpoint_metrics[["model", "rmse_carry_m", "mae_carry_m", "bias_carry_m", "carry_mape_pct", "rmse_peak_m", "rmse_flight_s"]].rename(columns={
        "rmse_carry_m": "Carry RMSE (m)", "mae_carry_m": "Carry MAE (m)", "bias_carry_m": "Carry 偏差 (m)",
        "carry_mape_pct": "Carry MAPE (%)", "rmse_peak_m": "最高点 RMSE (m)", "rmse_flight_s": "飞行时间 RMSE (s)"}))
    carry = initial[["shot_id", "reference_carry_m"]].set_index("shot_id")
    for model in MODELS[:2]:
        subset = endpoint[endpoint.model == model].set_index("shot_id")
        carry[f"{names[model]} Carry (m)"] = subset.predicted_carry_m
        carry[f"{names[model]} 误差 (m)"] = subset.error_carry_m
    carry = carry.reset_index().rename(columns={"shot_id": "Shot", "reference_carry_m": "Mevo D4 Carry (m)"})
    robust = snr_metrics.assign(sample="Shot 02–07，SNR ≥ 20（原始分值，非 dB）")
    robust = named(robust[robust.model != "mevo_d4"][["sample", "model", "n_points"] + list(radar_columns)].rename(columns={"sample": "敏感性口径", "n_points": "点数", **radar_columns}))
    s_table = named(sensitivity[sensitivity.model != "mevo_d4"][["model", "time_offset_s"] + list(radar_columns)].rename(columns={"time_offset_s": "共同偏移 (s)", **radar_columns}))
    r = overall.set_index("model")
    e = endpoint_metrics.set_index("model")
    fine_r = fine_metrics.set_index("model")
    shot_pivot = per_shot.pivot(index="shot_id", columns="model", values="rmse_vr_mps")
    improved_shots = int((shot_pivot.optimized < shot_pivot.original).sum())
    offset20 = sensitivity[sensitivity.time_offset_s.eq(.02)].set_index("model")
    offset_note = ("20 ms 共同偏移下总体排名轻微反转" if
                   offset20.loc['optimized', 'rmse_vr_mps'] > offset20.loc['original', 'rmse_vr_mps']
                   else "20 ms 共同偏移下优化模型径向速度 RMSE 仍较低，但优势随时间对齐变化")
    winners = ["优化模型" if r.loc["optimized", "rmse_" + key] < r.loc["original", "rmse_" + key] else "原始模型" for key in OBS]
    endpoint_winner = "优化模型" if e.loc["optimized", "rmse_carry_m"] < e.loc["original", "rmse_carry_m"] else "原始模型"
    maxconv = convergence.select_dtypes("number").drop(columns="shot_id").abs().max()
    text = f"""# Mevo+ 实测数据与 Libgolf 模型评估：默认环境、统一击球时刻

数据：`2026-09-19_091358Z`，**仅 Shot 02–07，共 6 杆**；原始 Libgolf 与冻结的 `filtered_665` 11 参数优化模型。未重新拟合参数。

## 1. 数据与统一设置

{table(input_table)}

按用户要求，因 Shot 01 的优化模型 Carry 相对误差为 10.58%，将其从两模型的全部评估、图表和汇总中排除，原始文件保留。**这是查看误差后的子集选择，不代表已证实 Shot 01 测量无效；结果仅适用于保留的 6 杆，不能解释为原始 7 杆的无偏性能。**

保留 1,233 个球雷达点；每杆首点用于视线锚定，不计入评分，共 **1,227 个评分点**，无其他删点。每杆实测约 0.63 s；`dist_m` 全为 0，不计算“实测三维位置 RMSE”。球雷达分页完整；`radarComplete=false` 涉及杆头分页失败，不等于球数据不完整。相机缺少有效标定，本报告不将其转换为三维真值。

| 设置 | 本次统一取值 |
| --- | --- |
| 温度 | 59 °F = 15 °C |
| 相对湿度 | 0% |
| 气压 | 29.92 inHg ≈ 1013.21 hPa |
| 海拔 | 0 ft = 0 m |
| 风速 | 0 mph = 0 m/s |
| 主结果击球时刻 / 雷达偏移 | 全部 `t₀=0 s`、`Δt=0 s`，`t_model=n` |
| 击球初始条件 | 每杆 final D4 的球速、发射仰角/方向、后旋/侧旋、起点；两模型完全相同 |
| 积分 / 落地 | 原库默认步长 0.01 s；首次穿越平地 Y=0 时线性插值，不含弹跳滚动 |

这里“统一”指同一杆的两模型共用同一套击球输入，并且所有杆采用同一个时间偏移，不是把不同杆的球速强行设成同值。环境直接使用 `AtmosphericData{{}}`，不是当日天气估计。输入取自同批观测解算出的 final D4，**这是给定初始条件的回放，不是仅使用击球瞬间已知信息的独立盲测**。总转速为后旋与侧旋平方和开根号，可能与设备整数总转速有舍入差。

**时间与坐标假设：** `n=0` 实际是首个球雷达点，暂作为击球时刻；没有独立证据证明两者严格重合。使用 `n`，不使用会回绕的设备 `time_tick`，也不把主机接收时间当击球时间。D4 的 `[前进, 高度, 侧向]` 对应 Libgolf 的 `[y,z,x]`；侧旋符号转换为 `libgolf_sidespin=-Mevo_sidespin`，保持侧向弯曲约定一致。侧旋转换全杆固定，不按残差选符号。

完整雷达外参缺失：假定雷达角度轴与 D4 轴同向，使用 `rangeMm=2438` 作为首点斜距估计，雷达位置取 `p_radar=p_start−2.438·u(az₀,el₀)`。各杆仅按同一规则建立原点，两模型共用，不拟合旋转、距离或角度偏置。`heightMm` 不当作传感器绝对高度。**所以角度/径向速度误差包含时间及外参假设误差，不是纯气动误差。**

## 2. 模型

| 模型 | 本次使用版本 |
| --- | --- |
| Libgolf 原始 | `DefaultAerodynamicModel`；原库阻力、Magnus 升力及自旋衰减 |
| 11 参数优化 | 原库基础上的分段阻力修正（Re、S）和全局升力修正（S），保留原自旋衰减；复用既有实现与冻结系数 |

优化系数来自 MyGolfSpy 1,640 条记录中、9 个来源构成的 `filtered_665` 聚合子集，以 Carry 为主并约束最高点误差。**本次 6 杆球速 28.91–36.85 m/s，全部低于该子集 39.47–71.93 m/s 的球速范围，是低速域外评估，不是原优化域的重复验证。**

## 3. 与早期雷达实测对比

比较视线径向速度、方位角、仰角。预测先投影到雷达视线：`v_r=(p−p_radar)·v/|p−p_radar|`，不能直接把球速模长与径向速度相减。角度误差折返到 ±180°；误差统一定义为预测减观测。

### 3.1 汇总误差

点加权 RMSE（每模型 1,227 点）：

{table(rtable)}

逐杆 RMSE 的算术平均（每杆等权，6 杆）：

{table(balance)}

Mevo D4 一行是将设备自己的拟合轨迹投影回同一雷达坐标的内部一致性参照，**不是第三套独立真值，也不是被评估的 Libgolf 模型**。1,227 个点在杆内高度相关，独立击球样本数只有 6，不给出虚假的大样本置信结论。

![逐杆雷达误差](figures/radar_rmse.png)

### 3.2 径向速度与角度曲线

黑点为原始雷达观测；蓝/橙线为两个模型；绿色虚线为 D4 拟合参照。所有曲线用同一时间与几何口径。

![径向速度](figures/radial_velocity.png)

![方位角和仰角](figures/radar_angles.png)

{table(named(shot_r))}

## 4. 与 Mevo D4 完整飞行结果对比

**本节是与设备拟合/外推结果的一致性对比，不等于实测落点精度。** Carry 采用 D4 原点到落点的水平距离 `hypot(X,Z)`；最高点与飞行时间分别取全程最大高度及首次落地时刻。D4 全程曲线为多项式，Carry 等表格参照值直接取最终 D4 摘要。

### 4.1 结果对比

{table(etable)}

{table(carry)}

![Carry 对比；散点按 shot CSV 来源着色](figures/carry_comparison.png)

![全程轨迹；绿色加粗段只是雷达窗口内的 D4 拟合，非三维实测](figures/full_trajectories.png)

### 4.2 各 Shot 的 D4 轨迹公式

以下直接使用各杆 `summary.json → flight → polyX / polyY / polyZ` 的最终 D4 四次多项式，并非本次重新拟合。系数数组按常数项、一次项至四次项排列，已解码为物理单位，**不再除以 `polyScale`**。

`t` 单位为秒；`X(t)` 为前进距离、`Y(t)` 为竖直高度、`Z(t)` 为侧向位置，单位均为米，坐标原点沿用 D4。常数项保留多项式原值，不强制替换为摘要中经过舍入的 `startPositionM`。系数仅在下列展示中保留 9 位小数，实际计算使用原始完整精度。

`t=0` 是 D4 模型的时间零点；本报告主对比假定 `t=n`，并非已独立确认真实撞击同步。各式仅在所列 D4 飞行时间内用于绘图，不应继续向外延长；雷达窗口后的曲线属于设备拟合／外推参考。摘要时间和系数存在量化、舍入，区间终点的 `Y(t)` 可能不严格等于 0。Shot 01 已排除，不在此列出。

{d4_formula_section(initial)}

## 5. 稳健性检查

### 5.1 统一时间偏移

固定几何及击球输入，所有杆、两模型统一测试 `t_model=n+Δt`，Δt 为 0 / 10 / 20 ms；**只做敏感性检查，不为每杆或每个模型寻找最优偏移**。10/20 ms 不是已确认的真实延迟，也未随偏移重新锚定首点。

{table(s_table)}

![共同时间偏移敏感性](figures/time_offset_sensitivity.png)

### 5.2 共同子集与数值步长

Shot 01 已排除；主结果保留 Shot 02–07 的全部非锚定点，不再按 SNR 删点。以下仅检查共同 SNR 掩码下结果是否变化。SNR 是原始整数，20 仅是检查门限，不宣称为设备质量合格线。

{table(robust)}

将步长从 0.01 s 改为 0.001 s，12 条轨迹的最大 Carry 差为 {maxconv.carry_delta_m:.4f} m、最高点差 {maxconv.peak_delta_m:.4f} m、飞行时间差 {maxconv.flight_delta_s:.5f} s；雷达采样时刻最大预测径向速度差 {maxconv.max_vr_delta_mps:.5f} m/s、方位角差 {maxconv.max_az_delta_deg:.5f}°、仰角差 {maxconv.max_el_delta_deg:.5f}°。细步长下原始/优化的径向速度 RMSE 为 {fine_r.loc['original', 'rmse_vr_mps']:.4f}/{fine_r.loc['optimized', 'rmse_vr_mps']:.4f} m/s。主报告保留原库默认步长结果。

## 6. 结论

- 仅 Shot 02–07，默认环境、统一零偏移下，径向速度 RMSE：原始 **{r.loc['original', 'rmse_vr_mps']:.3f}**、优化 **{r.loc['optimized', 'rmse_vr_mps']:.3f} m/s**；该指标{winners[0]}更低，但仅 {improved_shots}/{len(SHOTS)} 杆改善，{offset_note}。方位角 RMSE 以{winners[1]}更低，仰角以{winners[2]}更低，不能概括为全面优于原模型。
- 对 Mevo D4 的 Carry 参考，原始/优化 RMSE 为 **{e.loc['original', 'rmse_carry_m']:.3f} / {e.loc['optimized', 'rmse_carry_m']:.3f} m**，MAPE 为 **{e.loc['original', 'carry_mape_pct']:.2f}% / {e.loc['optimized', 'carry_mape_pct']:.2f}%**；{endpoint_winner}更接近该设备参考，但这不是独立落点验证。
- 这批数据可以做**低速域外的条件性回放评估**；目前不足以据此重拟合全部 11 个气动参数。修正前应先确认真实击球同步、雷达外参和现场环境，再补充独立 Carry/轨迹与更多球速工况；不要让气动系数吸收时序、坐标或设备外推偏差。

## 7. 复现与结果文件

需要 `g++`、`numpy`、`pandas`、`matplotlib`。在具备依赖的 Python 环境、仓库根目录运行：

```bash
python3 ironsight/mevo_data/2026-09-19_091358Z/libgolf_default_evaluation/evaluate_mevo_models.py
```

本次使用已有环境 `/home/mingruz/miniconda3/envs/rl_wsl_metadrive/bin/python`，默认 `python3` 环境缺少 pandas；详细软件版本与源文件校验值保存在 manifest。

| 文件 | 内容 |
| --- | --- |
| [initial_conditions.csv](outputs/initial_conditions.csv) | Shot 02–07 的 6 杆输入、环境、时间、坐标转换及雷达原点 |
| [radar_comparison.csv](outputs/radar_comparison.csv) | 零偏移逐点观测/预测/误差及评分掩码；3 组 × 1,233 行 |
| [overall_metrics.csv](outputs/overall_metrics.csv) / [radar_metrics_by_shot.csv](outputs/radar_metrics_by_shot.csv) | 雷达汇总及逐杆指标 |
| [trajectories.csv](outputs/trajectories.csv) | 两模型共 12 条完整预测轨迹 |
| [mevo_summary_comparison.csv](outputs/mevo_summary_comparison.csv) / [mevo_summary_metrics.csv](outputs/mevo_summary_metrics.csv) | D4 Carry / 最高点 / 飞行时间对比 |
| [time_offset_sensitivity.csv](outputs/time_offset_sensitivity.csv) / [snr20_sensitivity.csv](outputs/snr20_sensitivity.csv) | 时间与共同子集敏感性；旧兼容文件 shots_2_to_7_sensitivity.csv 现与主评估同口径 |
| [data_quality.csv](outputs/data_quality.csv) / [numerical_convergence.csv](outputs/numerical_convergence.csv) / [radar_metrics_dt_0p001.csv](outputs/radar_metrics_dt_0p001.csv) | 数据检查及步长检查 |
| [run_manifest.json](outputs/run_manifest.json) / [calibration.csv](outputs/calibration.csv) | 环境、假设、软件版本、源文件 SHA-256 与本次冻结系数 |

图片使用 `figures/` 相对路径；整个 `libgolf_default_evaluation` 文件夹可移动阅读。复算脚本仍需仓库模型源代码。本报告及其派生 CSV、图片已按 Shot 02–07 更新；原始测量文件（含 Shot 01）、已有模型参数和其他报告未修改。
"""
    (HERE / "mevo_libgolf_default_evaluation_report.md").write_text(text, encoding="utf-8")


if __name__ == "__main__":
    main()
