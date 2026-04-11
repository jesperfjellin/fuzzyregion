#!/usr/bin/env python3

"""Run the shipped tree-cover transition fuzzyregion demo and render a plot.

The script owns the complete public demo flow:

1. start an ephemeral PostgreSQL 18 + PostGIS container
2. install the fuzzyregion extension
3. load the shipped constructor inputs for two fuzzyregion values
4. query and compose fuzzyregion values in PostgreSQL
5. write the demo summary and an SVG plot
6. remove the container
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Callable


ROOT = Path(__file__).resolve().parents[1]
OUTPUT_DIR = ROOT / "examples" / "demo-output" / "po_valley_tree_cover_transition"
ANALYSIS_JSON = OUTPUT_DIR / "analysis.json"
PLOT_SVG = OUTPUT_DIR / "tree_cover_transition_demo.svg"

CONTAINER_NAME = "fuzzyregion-demo"
POSTGIS_IMAGE = "postgis/postgis:18-3.6"
POSTGRES_USER = "postgres"
POSTGRES_PASSWORD = "postgres"
POSTGRES_DB = "fuzzyregion_demo"
LOG_PREFIX = "[fuzzyregion:demo]"

# Figure styling: paper-like rather than dashboard-like.
WIDTH = 1280
HEIGHT = 810

BG = "#ffffff"
INK = "#111111"
MUTED = "#555555"
FRAME = "#8c8c8c"
LIGHT = "#d9d9d9"

MODERATE_COLORS = ["#f6dfc8", "#d7a16e", "#9d6231"]
HIGH_COLORS = ["#dbe8d3", "#8bae7a", "#4f7751"]
TRANSITION_COLORS = ["#dbe3f7", "#7c99d1", "#415f9e"]

UI_FONT = "Arial, Helvetica, sans-serif"
MONO_FONT = "Menlo, Consolas, monospace"

FIG_LEFT = 58
FIG_TOP = 90
PANEL_W = 545
PANEL_H = 285
COL_GAP = 38
ROW_GAP = 42

MAP_MARGIN_X = 14
MAP_MARGIN_TOP = 12
MAP_MARGIN_BOTTOM = 12
LEGEND_Y = 770


ANALYSIS_SQL = r"""
WITH inputs AS (
  SELECT moderate.area AS moderate_area, high.area AS high_area
  FROM fuzzyregion_demo.tree_cover_class AS moderate
  CROSS JOIN fuzzyregion_demo.tree_cover_class AS high
  WHERE moderate.class_label = 'moderate_tree_cover'
    AND high.class_label = 'high_tree_cover'
),
overlap AS (
  SELECT
    moderate_area,
    high_area,
    fuzzyregion_intersection(moderate_area, high_area) AS transition_area
  FROM inputs
)
SELECT json_build_object(
  'metrics', json_build_object(
    'moderate_levels', fuzzyregion_num_levels(moderate_area),
    'high_levels', fuzzyregion_num_levels(high_area),
    'transition_alpha_0_2_km2', round((fuzzyregion_area_at(transition_area, 0.2) / 1000000.0)::numeric, 2),
    'transition_alpha_0_4_km2', round((fuzzyregion_area_at(transition_area, 0.4) / 1000000.0)::numeric, 2),
    'transition_alpha_0_6_km2', round((fuzzyregion_area_at(transition_area, 0.6) / 1000000.0)::numeric, 2),
    'transition_levels', fuzzyregion_num_levels(transition_area),
    'min_alpha', fuzzyregion_min_alpha(transition_area),
    'max_alpha', fuzzyregion_max_alpha(transition_area)
  ),
  'moderate', json_build_object(
    'alpha_0_2', ST_AsGeoJSON(fuzzyregion_alpha_cut(moderate_area, 0.2), 3)::json,
    'alpha_0_4', ST_AsGeoJSON(fuzzyregion_alpha_cut(moderate_area, 0.4), 3)::json,
    'alpha_0_6', ST_AsGeoJSON(fuzzyregion_alpha_cut(moderate_area, 0.6), 3)::json
  ),
  'high', json_build_object(
    'alpha_0_2', ST_AsGeoJSON(fuzzyregion_alpha_cut(high_area, 0.2), 3)::json,
    'alpha_0_4', ST_AsGeoJSON(fuzzyregion_alpha_cut(high_area, 0.4), 3)::json,
    'alpha_0_6', ST_AsGeoJSON(fuzzyregion_alpha_cut(high_area, 0.6), 3)::json
  ),
  'transition', json_build_object(
    'alpha_0_2', ST_AsGeoJSON(fuzzyregion_alpha_cut(transition_area, 0.2), 3)::json,
    'alpha_0_4', ST_AsGeoJSON(fuzzyregion_alpha_cut(transition_area, 0.4), 3)::json,
    'alpha_0_6', ST_AsGeoJSON(fuzzyregion_alpha_cut(transition_area, 0.6), 3)::json
  )
)::text
FROM overlap;
"""

EXPORTS_SQL = r"""
WITH inputs AS (
  SELECT moderate.area AS moderate_area, high.area AS high_area
  FROM fuzzyregion_demo.tree_cover_class AS moderate
  CROSS JOIN fuzzyregion_demo.tree_cover_class AS high
  WHERE moderate.class_label = 'moderate_tree_cover'
    AND high.class_label = 'high_tree_cover'
),
overlap AS (
  SELECT
    moderate_area,
    high_area,
    fuzzyregion_intersection(moderate_area, high_area) AS transition_area
  FROM inputs
)
SELECT json_build_object(
  'moderate', json_build_object(
    'label', 'moderate_tree_cover',
    'stored_srid', fuzzyregion_srid(moderate_area),
    'export_srid', 4326,
    'text', fuzzyregion_to_text(moderate_area),
    'exact', fuzzyregion_levels(moderate_area),
    'geojson_levels', json_build_array(
      json_build_object('alpha', 0.2, 'geometry', ST_AsGeoJSON(ST_Transform(fuzzyregion_alpha_cut(moderate_area, 0.2), 4326), 6)::json),
      json_build_object('alpha', 0.4, 'geometry', ST_AsGeoJSON(ST_Transform(fuzzyregion_alpha_cut(moderate_area, 0.4), 4326), 6)::json),
      json_build_object('alpha', 0.6, 'geometry', ST_AsGeoJSON(ST_Transform(fuzzyregion_alpha_cut(moderate_area, 0.6), 4326), 6)::json)
    )
  ),
  'high', json_build_object(
    'label', 'high_tree_cover',
    'stored_srid', fuzzyregion_srid(high_area),
    'export_srid', 4326,
    'text', fuzzyregion_to_text(high_area),
    'exact', fuzzyregion_levels(high_area),
    'geojson_levels', json_build_array(
      json_build_object('alpha', 0.2, 'geometry', ST_AsGeoJSON(ST_Transform(fuzzyregion_alpha_cut(high_area, 0.2), 4326), 6)::json),
      json_build_object('alpha', 0.4, 'geometry', ST_AsGeoJSON(ST_Transform(fuzzyregion_alpha_cut(high_area, 0.4), 4326), 6)::json),
      json_build_object('alpha', 0.6, 'geometry', ST_AsGeoJSON(ST_Transform(fuzzyregion_alpha_cut(high_area, 0.6), 4326), 6)::json)
    )
  ),
  'transition', json_build_object(
    'label', 'transition_zone',
    'stored_srid', fuzzyregion_srid(transition_area),
    'export_srid', 4326,
    'text', fuzzyregion_to_text(transition_area),
    'exact', fuzzyregion_levels(transition_area),
    'geojson_levels', json_build_array(
      json_build_object('alpha', 0.2, 'geometry', ST_AsGeoJSON(ST_Transform(fuzzyregion_alpha_cut(transition_area, 0.2), 4326), 6)::json),
      json_build_object('alpha', 0.4, 'geometry', ST_AsGeoJSON(ST_Transform(fuzzyregion_alpha_cut(transition_area, 0.4), 4326), 6)::json),
      json_build_object('alpha', 0.6, 'geometry', ST_AsGeoJSON(ST_Transform(fuzzyregion_alpha_cut(transition_area, 0.6), 4326), 6)::json)
    )
  )
)::text
FROM overlap;
"""

TUPLE_DUMP_SQL = r"""
WITH inputs AS (
  SELECT moderate.area AS moderate_area, high.area AS high_area
  FROM fuzzyregion_demo.tree_cover_class AS moderate
  CROSS JOIN fuzzyregion_demo.tree_cover_class AS high
  WHERE moderate.class_label = 'moderate_tree_cover'
    AND high.class_label = 'high_tree_cover'
),
stored_rows AS (
  SELECT
    'fuzzyregion_demo.tree_cover_class' AS relation_name,
    id AS row_id,
    class_label AS label,
    area
  FROM fuzzyregion_demo.tree_cover_class
),
derived_rows AS (
  SELECT
    'derived' AS relation_name,
    NULL::integer AS row_id,
    'transition_zone' AS label,
    fuzzyregion_intersection(moderate_area, high_area) AS area
  FROM inputs
),
all_rows AS (
  SELECT * FROM stored_rows
  UNION ALL
  SELECT * FROM derived_rows
)
SELECT json_agg(
  json_build_object(
    'relation_name', relation_name,
    'row_id', row_id,
    'label', label,
    'srid', fuzzyregion_srid(area),
    'num_levels', fuzzyregion_num_levels(area),
    'min_alpha', fuzzyregion_min_alpha(area),
    'max_alpha', fuzzyregion_max_alpha(area),
    'levels', (
      SELECT json_agg(
        json_build_object(
          'alpha', level.value ->> 'alpha',
          'geometry_ewkb_hex_len', length(level.value ->> 'geometry_ewkb')
        )
        ORDER BY (level.value ->> 'alpha')::double precision DESC
      )
      FROM jsonb_array_elements(fuzzyregion_levels(area)::jsonb -> 'levels') AS level(value)
    )
  )
  ORDER BY label
)::text
FROM all_rows;
"""


@dataclass
class Bounds:
    min_x: float
    min_y: float
    max_x: float
    max_y: float

    def padded(self, fraction: float) -> "Bounds":
        width = self.max_x - self.min_x
        height = self.max_y - self.min_y
        pad_x = width * fraction
        pad_y = height * fraction
        return Bounds(
            self.min_x - pad_x,
            self.min_y - pad_y,
            self.max_x + pad_x,
            self.max_y + pad_y,
        )


def status(message: str) -> None:
    print(f"{LOG_PREFIX} {message}", flush=True)


def run(
    args: list[str],
    *,
    input_text: str | None = None,
    env: dict[str, str] | None = None,
    capture_output: bool = False,
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        args,
        input=input_text,
        text=True,
        env=env,
        capture_output=capture_output,
        check=True,
    )


def remove_demo_container() -> None:
    subprocess.run(
        ["docker", "rm", "-f", CONTAINER_NAME],
        text=True,
        capture_output=True,
        check=False,
    )


def start_container() -> None:
    status(f"Starting ephemeral demo container: {POSTGIS_IMAGE}")
    remove_demo_container()
    run(
        [
            "docker",
            "run",
            "--detach",
            "--name",
            CONTAINER_NAME,
            "--env",
            f"POSTGRES_USER={POSTGRES_USER}",
            "--env",
            f"POSTGRES_PASSWORD={POSTGRES_PASSWORD}",
            "--env",
            f"POSTGRES_DB={POSTGRES_DB}",
            POSTGIS_IMAGE,
        ],
        capture_output=True,
    )

    status("Waiting for PostgreSQL container initialization to complete.")
    for _ in range(90):
        logs = subprocess.run(
            ["docker", "logs", CONTAINER_NAME],
            text=True,
            capture_output=True,
            check=False,
        )
        if "PostgreSQL init process complete; ready for start up." in logs.stdout:
            break
        time.sleep(1)
    else:
        raise RuntimeError("PostgreSQL container did not finish initialization in time")

    run(
        [
            "docker",
            "exec",
            CONTAINER_NAME,
            "pg_isready",
            "-U",
            POSTGRES_USER,
            "-d",
            POSTGRES_DB,
        ],
        capture_output=True,
    )


def install_extension() -> None:
    status(f"Installing extension into {CONTAINER_NAME}.")
    env = {
        **os.environ,
        "FUZZYREGION_CONTAINER_NAME": CONTAINER_NAME,
        "FUZZYREGION_POSTGRES_USER": POSTGRES_USER,
        "FUZZYREGION_POSTGRES_DB": POSTGRES_DB,
    }
    run(
        [str(ROOT / "scripts" / "install-postgres-extension.sh")],
        env=env,
        capture_output=True,
    )


def load_demo_assets() -> None:
    status("Loading shipped constructor inputs for two fuzzyregion values.")
    env = {
        **os.environ,
        "FUZZYREGION_CONTAINER_NAME": CONTAINER_NAME,
        "FUZZYREGION_POSTGRES_USER": POSTGRES_USER,
        "FUZZYREGION_POSTGRES_DB": POSTGRES_DB,
    }
    run(
        [str(ROOT / "scripts" / "load-tree-cover-transition-demo.sh")],
        env=env,
        capture_output=True,
    )


def psql_query(sql: str) -> str:
    completed = run(
        [
            "docker",
            "exec",
            "-i",
            CONTAINER_NAME,
            "psql",
            "-X",
            "-tA",
            "-v",
            "ON_ERROR_STOP=1",
            "-U",
            POSTGRES_USER,
            "-d",
            POSTGRES_DB,
            "-f",
            "-",
        ],
        input_text=sql,
        capture_output=True,
    )
    return completed.stdout.strip()


def fetch_analysis() -> dict:
    status("Running fuzzyregion queries in PostgreSQL.")
    raw = psql_query(ANALYSIS_SQL)
    return json.loads(raw)


def fetch_exports() -> dict:
    status("Exporting tuple views for demo inspection.")
    raw = psql_query(EXPORTS_SQL)
    return json.loads(raw)


def fetch_tuple_dump() -> list[dict]:
    status("Reading concise tuple dump from PostgreSQL.")
    raw = psql_query(TUPLE_DUMP_SQL)
    return json.loads(raw)


def dump_diagnostics() -> None:
    try:
        ps = subprocess.run(
            ["docker", "ps", "-a", "--filter", f"name=^/{CONTAINER_NAME}$"],
            text=True,
            capture_output=True,
            check=False,
        )
        inspect = subprocess.run(
            [
                "docker",
                "inspect",
                CONTAINER_NAME,
                "--format",
                "status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} error={{.State.Error}}",
            ],
            text=True,
            capture_output=True,
            check=False,
        )
        logs = subprocess.run(
            ["docker", "logs", CONTAINER_NAME],
            text=True,
            capture_output=True,
            check=False,
        )
        sys.stderr.write("Demo run failed.\n")
        sys.stderr.write(ps.stdout)
        sys.stderr.write(inspect.stdout)
        sys.stderr.write("Container logs:\n")
        sys.stderr.write(logs.stdout)
        sys.stderr.write(logs.stderr)
    except Exception:
        pass


def geo_bounds(geometry: dict | None) -> Bounds | None:
    if not geometry:
        return None

    coords: list[tuple[float, float]] = []

    def visit(value: object) -> None:
        if isinstance(value, list):
            if value and isinstance(value[0], (int, float)):
                coords.append((float(value[0]), float(value[1])))
            else:
                for item in value:
                    visit(item)

    visit(geometry["coordinates"])

    if not coords:
        return None

    xs = [coord[0] for coord in coords]
    ys = [coord[1] for coord in coords]
    return Bounds(min(xs), min(ys), max(xs), max(ys))


def combine_bounds(bounds: list[Bounds]) -> Bounds:
    return Bounds(
        min(bound.min_x for bound in bounds),
        min(bound.min_y for bound in bounds),
        max(bound.max_x for bound in bounds),
        max(bound.max_y for bound in bounds),
    )


def svg_text(x: float, y: float, text: str, **attrs: str) -> str:
    attr_text = " ".join(f'{key}="{value}"' for key, value in attrs.items())
    escaped = (
        text.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace('"', "&quot;")
    )
    return f'<text x="{x:.1f}" y="{y:.1f}" {attr_text}>{escaped}</text>'


def svg_rect(x: float, y: float, width: float, height: float, **attrs: str) -> str:
    attr_text = " ".join(f'{key}="{value}"' for key, value in attrs.items())
    return (
        f'<rect x="{x:.1f}" y="{y:.1f}" width="{width:.1f}" '
        f'height="{height:.1f}" {attr_text} />'
    )


def panel_position(index: int) -> tuple[float, float]:
    if index == 0:
        return FIG_LEFT, FIG_TOP
    if index == 1:
        return FIG_LEFT + PANEL_W + COL_GAP, FIG_TOP
    if index == 2:
        return FIG_LEFT, FIG_TOP + PANEL_H + ROW_GAP
    if index == 3:
        return FIG_LEFT + PANEL_W + COL_GAP, FIG_TOP + PANEL_H + ROW_GAP
    raise ValueError(f"Unsupported panel index: {index}")


def build_transform(
    bounds: Bounds,
    x: float,
    y: float,
    width: float,
    height: float,
) -> Callable[[float, float], tuple[float, float]]:
    data_w = bounds.max_x - bounds.min_x
    data_h = bounds.max_y - bounds.min_y
    scale = min(width / data_w, height / data_h)

    offset_x = x + (width - data_w * scale) / 2
    offset_y = y + (height - data_h * scale) / 2

    def transform(px: float, py: float) -> tuple[float, float]:
        svg_x = offset_x + (px - bounds.min_x) * scale
        svg_y = offset_y + (bounds.max_y - py) * scale
        return svg_x, svg_y

    return transform


def geometry_path(
    geometry: dict | None,
    transform: Callable[[float, float], tuple[float, float]],
) -> str:
    if not geometry:
        return ""

    parts: list[str] = []
    if geometry["type"] == "Polygon":
        polygons = [geometry["coordinates"]]
    elif geometry["type"] == "MultiPolygon":
        polygons = geometry["coordinates"]
    else:
        raise ValueError(f"Unsupported geometry type: {geometry['type']}")

    for polygon in polygons:
        for ring in polygon:
            if not ring:
                continue
            first_x, first_y = transform(ring[0][0], ring[0][1])
            parts.append(f"M {first_x:.1f} {first_y:.1f}")
            for point in ring[1:]:
                x, y = transform(point[0], point[1])
                parts.append(f"L {x:.1f} {y:.1f}")
            parts.append("Z")

    return " ".join(parts)


def render_geometry_layer(
    geometry: dict | None,
    transform: Callable[[float, float], tuple[float, float]],
    fill: str,
    fill_opacity: str,
    stroke: str,
    stroke_width: str,
) -> str:
    path = geometry_path(geometry, transform)
    if not path:
        return ""
    return (
        f'<path d="{path}" fill="{fill}" fill-opacity="{fill_opacity}" '
        f'stroke="{stroke}" stroke-width="{stroke_width}" fill-rule="evenodd" />'
    )


def render_map_panel(
    index: int,
    panel_label: str,
    title: str,
    layers: list[tuple[dict | None, str, str, str, str]],
    bounds: Bounds,
) -> str:
    px, py = panel_position(index)

    map_x = px + MAP_MARGIN_X
    map_y = py + MAP_MARGIN_TOP
    map_w = PANEL_W - (2 * MAP_MARGIN_X)
    map_h = PANEL_H - MAP_MARGIN_TOP - MAP_MARGIN_BOTTOM

    transform = build_transform(bounds, map_x, map_y, map_w, map_h)
    clip_id = f"map-clip-{index}"

    parts = [
        svg_text(px, py - 12, f"{panel_label} {title}", fill=INK, **{
            "font-size": "18",
            "font-family": UI_FONT,
            "font-weight": "700",
        }),
        svg_rect(px, py, PANEL_W, PANEL_H, fill="#ffffff", stroke=FRAME, **{
            "stroke-width": "1.0",
        }),
        f'<defs><clipPath id="{clip_id}">{svg_rect(map_x, map_y, map_w, map_h)}</clipPath></defs>',
        svg_rect(map_x, map_y, map_w, map_h, fill="#ffffff", stroke=LIGHT, **{
            "stroke-width": "0.8",
        }),
    ]

    rendered_layers: list[str] = []
    for geometry, fill, fill_opacity, stroke, stroke_width in layers:
        rendered = render_geometry_layer(
            geometry,
            transform,
            fill,
            fill_opacity,
            stroke,
            stroke_width,
        )
        if rendered:
            rendered_layers.append(rendered)

    if rendered_layers:
        parts.append(f'<g clip-path="url(#{clip_id})">{"".join(rendered_layers)}</g>')

    return "\n".join(parts)


def render_metrics_panel(metrics: dict) -> str:
    px, py = panel_position(3)

    moderate_levels = int(metrics["moderate_levels"])
    high_levels = int(metrics["high_levels"])
    transition_levels = int(metrics["transition_levels"])
    alpha_0_2 = float(metrics["transition_alpha_0_2_km2"])
    alpha_0_4 = float(metrics["transition_alpha_0_4_km2"])
    alpha_0_6 = float(metrics["transition_alpha_0_6_km2"])

    rows = [
        ("Moderate class", f"fuzzyregion / {moderate_levels} levels"),
        ("High class", f"fuzzyregion / {high_levels} levels"),
        ("Intersection result", f"fuzzyregion / {transition_levels} levels"),
        ("Transition area at α ≥ 0.2", f"{alpha_0_2:,.2f} km²"),
        ("Transition area at α ≥ 0.4", f"{alpha_0_4:,.2f} km²"),
        ("Transition area at α ≥ 0.6", f"{alpha_0_6:,.2f} km²"),
    ]

    y0 = py + 40
    row_h = 32

    parts = [
        svg_text(px, py - 12, "d SQL readout", fill=INK, **{
            "font-size": "18",
            "font-family": UI_FONT,
            "font-weight": "700",
        }),
        svg_rect(px, py, PANEL_W, PANEL_H, fill="#ffffff", stroke=FRAME, **{
            "stroke-width": "1.0",
        }),
        svg_text(px + 18, py + 24, "Two overlapping classes from one raster, plus their fuzzy intersection.", fill=MUTED, **{
            "font-size": "12",
            "font-family": UI_FONT,
        }),
    ]

    for i, (label, value) in enumerate(rows):
        y = y0 + i * row_h
        parts.append(
            f'<line x1="{px + 16:.1f}" y1="{y + 10:.1f}" '
            f'x2="{px + PANEL_W - 16:.1f}" y2="{y + 10:.1f}" '
            f'stroke="{LIGHT}" stroke-width="0.8" />'
        )
        parts.append(svg_text(px + 18, y + 30, label, fill=INK, **{
            "font-size": "13",
            "font-family": UI_FONT,
        }))
        parts.append(svg_text(px + PANEL_W - 18, y + 30, value, fill=INK, **{
            "font-size": "13",
            "font-family": MONO_FONT,
            "text-anchor": "end",
        }))

    parts.append(svg_text(px + 18, py + PANEL_H - 42, f"Transition α range: {metrics['min_alpha']}–{metrics['max_alpha']}", fill=MUTED, **{
        "font-size": "12",
        "font-family": UI_FONT,
    }))
    parts.append(svg_text(px + 18, py + PANEL_H - 22, "All panels share the same spatial extent; colors encode increasing alpha.", fill=MUTED, **{
        "font-size": "12",
        "font-family": UI_FONT,
    }))

    return "\n".join(parts)


def render_legend() -> str:
    items = [
        ("Moderate tree cover", MODERATE_COLORS),
        ("High tree cover", HIGH_COLORS),
        ("Transition zone", TRANSITION_COLORS),
    ]

    x = FIG_LEFT
    y = LEGEND_Y

    parts = [
        f'<line x1="{FIG_LEFT:.1f}" y1="{y - 18:.1f}" x2="{WIDTH - FIG_LEFT:.1f}" y2="{y - 18:.1f}" stroke="{LIGHT}" stroke-width="0.8" />',
        svg_text(x, y, "Legend", fill=INK, **{
            "font-size": "13",
            "font-family": UI_FONT,
            "font-weight": "700",
        }),
    ]

    x += 62
    for label, colors in items:
        parts.append(svg_text(x, y, label, fill=INK, **{
            "font-size": "12",
            "font-family": UI_FONT,
        }))
        x += 82
        for alpha_label, color in zip(("0.2", "0.4", "0.6"), colors):
            parts.append(svg_rect(x, y - 10, 14, 14, fill=color, stroke="#666666", **{
                "stroke-width": "0.3",
            }))
            parts.append(svg_text(x + 20, y + 1, alpha_label, fill=MUTED, **{
                "font-size": "11",
                "font-family": MONO_FONT,
            }))
            x += 52
        x += 28

    return "\n".join(parts)


def render_svg(analysis: dict) -> str:
    geometry_bounds_list = [
        geo_bounds(analysis["moderate"]["alpha_0_2"]),
        geo_bounds(analysis["high"]["alpha_0_2"]),
        geo_bounds(analysis["transition"]["alpha_0_2"]),
    ]
    bounds = combine_bounds([bound for bound in geometry_bounds_list if bound is not None]).padded(0.04)

    moderate_panel = render_map_panel(
        0,
        "a",
        "Moderate tree cover",
        [
            (analysis["moderate"]["alpha_0_2"], MODERATE_COLORS[0], "0.45", "#c49a73", "0.6"),
            (analysis["moderate"]["alpha_0_4"], MODERATE_COLORS[1], "0.62", "#9f6b3e", "0.7"),
            (analysis["moderate"]["alpha_0_6"], MODERATE_COLORS[2], "0.78", "#7c4b1f", "0.8"),
        ],
        bounds,
    )

    high_panel = render_map_panel(
        1,
        "b",
        "High tree cover",
        [
            (analysis["high"]["alpha_0_2"], HIGH_COLORS[0], "0.45", "#94aa8e", "0.6"),
            (analysis["high"]["alpha_0_4"], HIGH_COLORS[1], "0.62", "#6f8f69", "0.7"),
            (analysis["high"]["alpha_0_6"], HIGH_COLORS[2], "0.78", "#4a6f4d", "0.8"),
        ],
        bounds,
    )

    transition_panel = render_map_panel(
        2,
        "c",
        "Transition zone (moderate ∩ high)",
        [
            (analysis["transition"]["alpha_0_2"], TRANSITION_COLORS[0], "0.58", "#8ea7cf", "0.8"),
            (analysis["transition"]["alpha_0_4"], TRANSITION_COLORS[1], "0.70", "#587dbf", "0.9"),
            (analysis["transition"]["alpha_0_6"], TRANSITION_COLORS[2], "0.88", "#274985", "1.0"),
        ],
        bounds,
    )

    metrics_panel = render_metrics_panel(analysis["metrics"])

    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{HEIGHT}" viewBox="0 0 {WIDTH} {HEIGHT}">
  <rect width="100%" height="100%" fill="{BG}" />
  {svg_text(FIG_LEFT, 40, 'Fuzzyregion in PostgreSQL: One Tree-Cover Raster, Two Stored Classes', fill=INK, **{'font-size': '24', 'font-weight': '700', 'font-family': UI_FONT})}
  {svg_text(FIG_LEFT, 63, 'Panels a and b are overlapping fuzzyregion classes derived from one tree-cover raster; panel c is the fuzzyregion returned by fuzzyregion_intersection. The source dataset is only scaffolding for the type demo.', fill=MUTED, **{'font-size': '13', 'font-family': UI_FONT})}
  {moderate_panel}
  {high_panel}
  {transition_panel}
  {metrics_panel}
  {render_legend()}
</svg>
"""


def build_levels_geojson(export_entry: dict) -> dict:
    features = []
    label = export_entry["label"]
    stored_srid = export_entry["stored_srid"]
    export_srid = export_entry["export_srid"]

    for level in export_entry["geojson_levels"]:
        geometry = level.get("geometry")
        if not geometry:
            continue

        features.append(
            {
                "type": "Feature",
                "properties": {
                    "label": label,
                    "alpha": float(level["alpha"]),
                    "stored_srid": stored_srid,
                    "geojson_srid": export_srid,
                    "source": "fuzzyregion_demo_export",
                },
                "geometry": geometry,
            }
        )

    return {
        "type": "FeatureCollection",
        "name": label,
        "features": features,
    }


def write_demo_exports(exports: dict) -> list[Path]:
    exported_paths: list[Path] = []
    for key in ("moderate", "high", "transition"):
        export = exports[key]
        label = export["label"]
        exact_payload = {
            "type": "fuzzyregion_demo_export",
            "label": label,
            "source": "run_demo.py",
            "representation": export["exact"],
            "text": export["text"],
        }

        exact_path = OUTPUT_DIR / f"{label}.fuzzyregion.json"
        exact_path.write_text(json.dumps(exact_payload, indent=2), encoding="utf-8")
        exported_paths.append(exact_path)

        levels_geojson = build_levels_geojson(export)
        levels_path = OUTPUT_DIR / f"{label}.levels.geojson"
        levels_path.write_text(json.dumps(levels_geojson, indent=2), encoding="utf-8")
        exported_paths.append(levels_path)

    return exported_paths


def write_outputs(analysis: dict, exports: dict | None = None) -> None:
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    ANALYSIS_JSON.write_text(json.dumps(analysis, indent=2), encoding="utf-8")
    PLOT_SVG.write_text(render_svg(analysis), encoding="utf-8")
    if exports is not None:
        write_demo_exports(exports)


def print_summary(analysis: dict, exports: dict | None = None) -> None:
    metrics = analysis["metrics"]
    status("Demo completed.")
    print(
        f"  moderate class:   fuzzyregion ({metrics['moderate_levels']} levels)\n"
        f"  high class:       fuzzyregion ({metrics['high_levels']} levels)\n"
        f"  intersection:     fuzzyregion ({metrics['transition_levels']} levels, {metrics['min_alpha']} .. {metrics['max_alpha']})\n"
        f"  transition α≥0.2: {metrics['transition_alpha_0_2_km2']} km²\n"
        f"  transition α≥0.4: {metrics['transition_alpha_0_4_km2']} km²\n"
        f"  transition α≥0.6: {metrics['transition_alpha_0_6_km2']} km²"
    )
    print(f"  demo json:        {ANALYSIS_JSON}")
    print(f"  plot svg:         {PLOT_SVG}")
    if exports is not None:
        print(f"  tuple export:     {OUTPUT_DIR / (exports['moderate']['label'] + '.fuzzyregion.json')}")
        print(f"  tuple export:     {OUTPUT_DIR / (exports['high']['label'] + '.fuzzyregion.json')}")
        print(f"  tuple export:     {OUTPUT_DIR / (exports['transition']['label'] + '.fuzzyregion.json')}")


def print_tuple_dump(tuple_dump: list[dict]) -> None:
    print("  raw tuple dump:")
    for row in tuple_dump:
        row_id = row["row_id"]
        row_prefix = (
            f"{row['relation_name']}(id={row_id}, label='{row['label']}', "
            if row_id is not None
            else f"{row['relation_name']}(label='{row['label']}', "
        )
        print(
            "    "
            f"{row_prefix}area=fuzzyregion{{srid={row['srid']}, "
            f"levels={row['num_levels']}, alpha_range={row['min_alpha']}..{row['max_alpha']}}})"
        )
        for level in row["levels"]:
            print(
                "      "
                f"level alpha={level['alpha']} ewkb_hex_len={level['geometry_ewkb_hex_len']}"
            )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--analysis-json",
        type=Path,
        help="Render the SVG from an existing analysis JSON file without running Docker.",
    )
    args = parser.parse_args()

    if args.analysis_json is not None:
        analysis = json.loads(args.analysis_json.read_text(encoding="utf-8"))
        write_outputs(analysis)
        print_summary(analysis)
        return 0

    remove_demo_container()
    try:
        start_container()
        install_extension()
        load_demo_assets()
        analysis = fetch_analysis()
        exports = fetch_exports()
        tuple_dump = fetch_tuple_dump()
        write_outputs(analysis, exports)
        print_summary(analysis, exports)
        print_tuple_dump(tuple_dump)
        return 0
    except subprocess.CalledProcessError as error:
        sys.stderr.write(f"{LOG_PREFIX} command failed: {' '.join(error.cmd)}\n")
        if error.stdout:
            sys.stderr.write(error.stdout)
        if error.stderr:
            sys.stderr.write(error.stderr)
        dump_diagnostics()
        return 1
    except Exception as error:
        sys.stderr.write(f"{LOG_PREFIX} {error}\n")
        dump_diagnostics()
        return 1
    finally:
        remove_demo_container()


if __name__ == "__main__":
    sys.exit(main())
