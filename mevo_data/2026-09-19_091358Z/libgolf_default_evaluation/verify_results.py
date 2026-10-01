#!/usr/bin/env python3
"""Independent checks of saved results, relative links, source hashes and launch units."""
import hashlib
import json
from pathlib import Path
import re

import numpy as np
import pandas as pd
from PIL import Image

HERE = Path(__file__).resolve().parent
ROOT = next(p for p in HERE.parents if (p / "offline_model/data_and_model/model/libgolf").is_dir())
OUT = HERE / "outputs"
manifest = json.loads((OUT / "run_manifest.json").read_text())
for name, expected in manifest["sha256"].items():
    assert hashlib.sha256((ROOT / name).read_bytes()).hexdigest() == expected, name

initial = pd.read_csv(OUT / "initial_conditions.csv").set_index("shot_id")
trajectory = pd.read_csv(OUT / "trajectories.csv")
points = pd.read_csv(OUT / "radar_comparison.csv")
overall = pd.read_csv(OUT / "overall_metrics.csv").set_index("model")
per_shot = pd.read_csv(OUT / "radar_metrics_by_shot.csv").set_index(["model", "shot_id"])
endpoints = pd.read_csv(OUT / "mevo_summary_comparison.csv").set_index(["shot_id", "model"])
assert set(initial.index) == set(range(2, 8))
assert len(initial) == 6 and len(points) == 3 * 1233
assert len(endpoints) == 12
assert manifest['included_shot_ids'] == list(range(2, 8)) and manifest['excluded_shot_ids'] == [1]
for path in OUT.glob('*.csv'):
    frame = pd.read_csv(path)
    if 'shot_id' in frame:
        assert set(frame.shot_id) == set(range(2, 8)), path.name
assert initial.impact_time_s.eq(0).all() and initial.radar_time_offset_s.eq(0).all()
np.testing.assert_array_equal(initial.libgolf_sidespin_rpm, -initial.mevo_sidespin_rpm)
for model, group in points.groupby("model"):
    assert len(group) == 1233 and group.evaluation_mask.sum() == 1227
    assert not group.duplicated(["shot_id", "point_id"]).any()
    selected = group[group.evaluation_mask]
    for obs in ["vr_mps", "az_deg", "el_deg"]:
        error = group[f"predicted_{obs}"].to_numpy() - group[f"observed_{obs}"].to_numpy()
        if obs != "vr_mps":
            error = (error + 180) % 360 - 180
        np.testing.assert_allclose(error, group[f"error_{obs}"], atol=1e-12)
        expected = np.sqrt(np.mean(selected[f"error_{obs}"].to_numpy() ** 2))
        np.testing.assert_allclose(expected, overall.loc[model, f"rmse_{obs}"], atol=1e-12)
        for sid, shot in selected.groupby("shot_id"):
            expected = np.sqrt(np.mean(shot[f"error_{obs}"].to_numpy() ** 2))
            np.testing.assert_allclose(expected, per_shot.loc[(model, sid), f"rmse_{obs}"], atol=1e-12)
    for sid, shot in group.groupby("shot_id"):
        raw = pd.read_csv(HERE.parent / f"shot_{sid:06d}/radar_ball_raw.csv")
        for out_col, raw_col in [("observed_vr_mps", "radial_velocity_mps"), ("observed_az_deg", "az_deg"),
                                  ("observed_el_deg", "el_deg"), ("n", "n")]:
            np.testing.assert_array_equal(shot[out_col], raw[raw_col])

for (sid, model), group in trajectory.groupby(["shot_id", "model"]):
    first, last = group.iloc[0], group.iloc[-1]
    launch = initial.loc[sid]
    az, el = np.radians([launch.azimuth_deg, launch.elevation_deg])
    expected_v = launch.speed_mps * np.array([np.cos(el) * np.cos(az), np.sin(el), np.cos(el) * np.sin(az)])
    np.testing.assert_allclose(first[["VX_mps", "VY_mps", "VZ_mps"]].to_numpy(dtype=float), expected_v, atol=1e-5, rtol=1e-6)
    np.testing.assert_allclose(first[["X_m", "Y_m", "Z_m"]].to_numpy(dtype=float),
                               launch[["start_X_m", "start_Y_m", "start_Z_m"]].to_numpy(dtype=float), atol=1e-7)
    assert first.t_s == 0 and last.Y_m == 0 and (group.Y_m.iloc[:-1] > 0).all()
    endpoint = endpoints.loc[(sid, model)]
    np.testing.assert_allclose(np.hypot(last.X_m, last.Z_m), endpoint.predicted_carry_m, atol=1e-10)
    np.testing.assert_allclose(last.t_s, endpoint.predicted_flight_s, atol=1e-10)
    anchor = points[(points.shot_id == sid) & (points.model == model)].iloc[0]
    np.testing.assert_allclose([anchor.error_az_deg, anchor.error_el_deg], [0, 0], atol=1e-5)

report = (HERE / "mevo_libgolf_default_evaluation_report.md").read_text()
formula_blocks = re.findall(r"#### Shot (\d+)\n(.*?)(?=\n#### |\n## |\Z)", report, flags=re.S)
assert [int(sid) for sid, _ in formula_blocks] == list(range(2, 8))
for sid, block in formula_blocks:
    flight = json.loads((HERE.parent / f"shot_{int(sid):06d}/summary.json").read_text())["flight"]
    for axis in "XYZ":
        expression = re.search(rf"^{axis}\(t\) &= (.+)$", block, flags=re.M).group(1)
        rounded = np.zeros(5)
        for sign, value, factor, power in re.findall(r"([+-]?)\s*(\d+\.\d+)(t(?:\^\{(\d)\})?)?", expression):
            index = int(power) if power else (1 if factor else 0)
            rounded[index] = float(value) * (-1 if sign == "-" else 1)
        np.testing.assert_allclose(rounded, flight[f"poly{axis}"], atol=5.01e-10, rtol=0)
        times = np.linspace(0, flight["flightTimeSeconds"], 501)
        np.testing.assert_allclose(np.polynomial.polynomial.polyval(times, rounded),
                                   np.polynomial.polynomial.polyval(times, flight[f"poly{axis}"]),
                                   atol=5e-7, rtol=0)
links = re.findall(r"!?\[[^\]]*\]\(([^)]+)\)", report)
assert len(re.findall(r"!\[", report)) == 6
for link in links:
    assert not Path(link).is_absolute() and not link.startswith(("data:", "http:", "file:")), link
    path = HERE / link
    assert path.is_file(), link
    if path.suffix == ".png":
        with Image.open(path) as im:
            im.verify()
print(f"PASS: {len(manifest['sha256'])} source hashes; 6 shots (2-7); 12 trajectories; "
      f"3,699 comparison rows; Shot 1 excluded from all per-shot outputs; exact source observations; independent metric/launch checks; 18 D4 equations; {len(links)} relative links; 6 PNGs.")
