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
import base64
import json
import math
import os
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Callable

import requests


ROOT = Path(__file__).resolve().parents[1]
OUTPUT_DIR = ROOT / "examples" / "demo-output" / "po_valley_tree_cover_transition"
ANALYSIS_JSON = OUTPUT_DIR / "analysis.json"
PLOT_SVG = OUTPUT_DIR / "tree_cover_transition_demo.svg"
OSM_CACHE_DIR = OUTPUT_DIR / "osm-standard-cache"

CONTAINER_NAME = "fuzzyregion-demo"
POSTGIS_IMAGE = "postgis/postgis:18-3.6"
POSTGRES_USER = "postgres"
POSTGRES_PASSWORD = "postgres"
POSTGRES_DB = "fuzzyregion_demo"
LOG_PREFIX = "[fuzzyregion:demo]"
OSM_TILE_TEMPLATE = "https://tile.openstreetmap.org/{z}/{x}/{y}.png"
OSM_USER_AGENT = "fuzzyregion-demo/0.1 (+https://github.com/jespe/fuzzyregion)"
OSM_ATTRIBUTION = "Basemap © OpenStreetMap contributors"
WEB_MERCATOR_LIMIT = 20037508.342789244

# Figure styling: paper-like rather than dashboard-like.
WIDTH = 1280
HEIGHT = 700

BG = "#ffffff"
INK = "#111111"
MUTED = "#555555"
FRAME = "#8c8c8c"
LIGHT = "#d9d9d9"

ALPHA_COLORS = ["#55a868", "#e3b505", "#c83e3a"]

UI_FONT = "Arial, Helvetica, sans-serif"
TITLE_FONT = "Georgia, 'Times New Roman', serif"
MONO_FONT = "Menlo, Consolas, monospace"

FIG_LEFT = 42
FIG_TOP = 92
PANEL_W = 382
PANEL_H = 430
COL_GAP = 25

MAP_MARGIN_X = 10
MAP_MARGIN_TOP = 42
MAP_MARGIN_BOTTOM = 12
LEGEND_Y = 656


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
    'alpha_0_2', ST_AsGeoJSON(ST_Transform(fuzzyregion_alpha_cut(moderate_area, 0.2), 3857), 3)::json,
    'alpha_0_4', ST_AsGeoJSON(ST_Transform(fuzzyregion_alpha_cut(moderate_area, 0.4), 3857), 3)::json,
    'alpha_0_6', ST_AsGeoJSON(ST_Transform(fuzzyregion_alpha_cut(moderate_area, 0.6), 3857), 3)::json
  ),
  'high', json_build_object(
    'alpha_0_2', ST_AsGeoJSON(ST_Transform(fuzzyregion_alpha_cut(high_area, 0.2), 3857), 3)::json,
    'alpha_0_4', ST_AsGeoJSON(ST_Transform(fuzzyregion_alpha_cut(high_area, 0.4), 3857), 3)::json,
    'alpha_0_6', ST_AsGeoJSON(ST_Transform(fuzzyregion_alpha_cut(high_area, 0.6), 3857), 3)::json
  ),
  'transition', json_build_object(
    'alpha_0_2', ST_AsGeoJSON(ST_Transform(fuzzyregion_alpha_cut(transition_area, 0.2), 3857), 3)::json,
    'alpha_0_4', ST_AsGeoJSON(ST_Transform(fuzzyregion_alpha_cut(transition_area, 0.4), 3857), 3)::json,
    'alpha_0_6', ST_AsGeoJSON(ST_Transform(fuzzyregion_alpha_cut(transition_area, 0.6), 3857), 3)::json
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
    status(f"Starting demo container: {POSTGIS_IMAGE}")
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


def polygon_bounds(polygon: list[list[list[float]]]) -> Bounds | None:
    coords = [point for ring in polygon for point in ring]
    if not coords:
        return None
    xs = [float(point[0]) for point in coords]
    ys = [float(point[1]) for point in coords]
    return Bounds(min(xs), min(ys), max(xs), max(ys))


def ring_area(ring: list[list[float]]) -> float:
    if len(ring) < 3:
        return 0.0
    area = 0.0
    for i in range(len(ring)):
        x1, y1 = float(ring[i][0]), float(ring[i][1])
        x2, y2 = float(ring[(i + 1) % len(ring)][0]), float(ring[(i + 1) % len(ring)][1])
        area += (x1 * y2) - (x2 * y1)
    return abs(area) / 2.0


def largest_polygon_bounds(geometry: dict | None) -> Bounds | None:
    if not geometry:
        return None

    if geometry["type"] == "Polygon":
        polygons = [geometry["coordinates"]]
    elif geometry["type"] == "MultiPolygon":
        polygons = geometry["coordinates"]
    else:
        return None

    best_bounds: Bounds | None = None
    best_area = -1.0
    for polygon in polygons:
        if not polygon or not polygon[0]:
            continue
        area = ring_area(polygon[0])
        bounds = polygon_bounds(polygon)
        if bounds is None:
            continue
        if area > best_area:
            best_area = area
            best_bounds = bounds
    return best_bounds


def focus_bounds(analysis: dict) -> Bounds:
    for key in ("alpha_0_6", "alpha_0_4", "alpha_0_2"):
        bounds = largest_polygon_bounds(analysis["transition"][key])
        if bounds is not None:
            return bounds.padded(0.35)

    bounds_list = [
        geo_bounds(analysis["moderate"]["alpha_0_2"]),
        geo_bounds(analysis["high"]["alpha_0_2"]),
        geo_bounds(analysis["transition"]["alpha_0_2"]),
    ]
    present_bounds = [bound for bound in bounds_list if bound is not None]
    return combine_bounds(present_bounds).padded(0.06) if present_bounds else Bounds(0, 0, 1, 1)


def clamp_mercator(bounds: Bounds) -> Bounds:
    return Bounds(
        max(-WEB_MERCATOR_LIMIT, bounds.min_x),
        max(-WEB_MERCATOR_LIMIT, bounds.min_y),
        min(WEB_MERCATOR_LIMIT, bounds.max_x),
        min(WEB_MERCATOR_LIMIT, bounds.max_y),
    )


def mercator_tile_range(bounds: Bounds, zoom: int) -> tuple[int, int, int, int]:
    bounds = clamp_mercator(bounds)
    n = 2**zoom
    world = WEB_MERCATOR_LIMIT * 2.0

    def x_index(x: float) -> int:
        return max(0, min(n - 1, int(math.floor(((x + WEB_MERCATOR_LIMIT) / world) * n))))

    def y_index(y: float) -> int:
        return max(0, min(n - 1, int(math.floor(((WEB_MERCATOR_LIMIT - y) / world) * n))))

    x_min = x_index(bounds.min_x)
    x_max = x_index(bounds.max_x)
    y_min = y_index(bounds.max_y)
    y_max = y_index(bounds.min_y)
    return x_min, x_max, y_min, y_max


def mercator_tile_bounds(zoom: int, x: int, y: int) -> Bounds:
    n = 2**zoom
    world = WEB_MERCATOR_LIMIT * 2.0
    min_x = (x / n) * world - WEB_MERCATOR_LIMIT
    max_x = ((x + 1) / n) * world - WEB_MERCATOR_LIMIT
    max_y = WEB_MERCATOR_LIMIT - (y / n) * world
    min_y = WEB_MERCATOR_LIMIT - ((y + 1) / n) * world
    return Bounds(min_x, min_y, max_x, max_y)


def choose_osm_zoom(bounds: Bounds) -> int:
    for zoom in range(12, 7, -1):
        x_min, x_max, y_min, y_max = mercator_tile_range(bounds, zoom)
        x_count = x_max - x_min + 1
        y_count = y_max - y_min + 1
        total = x_count * y_count
        if x_count <= 5 and y_count <= 5 and total <= 20:
            return zoom
    return 8


def fetch_osm_tile(zoom: int, x: int, y: int) -> bytes | None:
    cache_path = OSM_CACHE_DIR / str(zoom) / str(x) / f"{y}.png"
    if cache_path.exists():
        return cache_path.read_bytes()

    url = OSM_TILE_TEMPLATE.format(z=zoom, x=x, y=y)
    for attempt in range(3):
        try:
            response = requests.get(
                url,
                headers={"User-Agent": OSM_USER_AGENT},
                timeout=20,
            )
            response.raise_for_status()
            break
        except requests.RequestException:
            if attempt == 2:
                return None
            time.sleep(0.5)

    cache_path.parent.mkdir(parents=True, exist_ok=True)
    cache_path.write_bytes(response.content)
    return response.content


def build_osm_basemap(bounds: Bounds) -> list[tuple[Bounds, str]]:
    padded_bounds = clamp_mercator(bounds.padded(0.08))
    zoom = choose_osm_zoom(padded_bounds)
    x_min, x_max, y_min, y_max = mercator_tile_range(padded_bounds, zoom)
    n = 2**zoom
    x_min = max(0, x_min - 1)
    x_max = min(n - 1, x_max + 1)
    y_min = max(0, y_min - 1)
    y_max = min(n - 1, y_max + 1)

    tiles: list[tuple[Bounds, str]] = []
    for x in range(x_min, x_max + 1):
        for y in range(y_min, y_max + 1):
            tile = fetch_osm_tile(zoom, x, y)
            if tile is None:
                continue
            encoded = base64.b64encode(tile).decode("ascii")
            tiles.append((mercator_tile_bounds(zoom, x, y), encoded))
    return tiles


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


def render_alpha_object_panel(
    x: float,
    y: float,
    width: float,
    height: float,
    caption: str,
    layers: list[tuple[dict | None, str, str, str, str]],
    basemap_tiles: list[tuple[Bounds, str]],
    bounds: Bounds,
    clip_id: str,
) -> str:
    map_x = x + MAP_MARGIN_X
    map_y = y + MAP_MARGIN_TOP
    map_w = width - (2 * MAP_MARGIN_X)
    map_h = height - MAP_MARGIN_TOP - MAP_MARGIN_BOTTOM

    transform = build_transform(bounds, map_x, map_y, map_w, map_h)

    rendered_layers: list[str] = []
    basemap_images: list[str] = []
    for tile_bounds, encoded_tile in basemap_tiles:
        x1, y1 = transform(tile_bounds.min_x, tile_bounds.max_y)
        x2, y2 = transform(tile_bounds.max_x, tile_bounds.min_y)
        basemap_images.append(
            f'<image x="{x1:.1f}" y="{y1:.1f}" width="{(x2 - x1):.1f}" height="{(y2 - y1):.1f}" '
            f'preserveAspectRatio="none" opacity="0.78" href="data:image/png;base64,{encoded_tile}" />'
        )

    for geometry, fill, opacity, stroke, stroke_width in layers:
        rendered = render_geometry_layer(
            geometry,
            transform,
            fill,
            opacity,
            stroke,
            stroke_width,
        )
        if rendered:
            rendered_layers.append(rendered)

    parts = [
        svg_rect(x, y, width, height, fill="#ffffff", stroke=FRAME, **{
            "stroke-width": "1.0",
        }),
        svg_text(x + 12, y + 24, caption, fill=INK, **{
            "font-size": "14",
            "font-family": UI_FONT,
            "font-weight": "700",
        }),
        f'<line x1="{x + 10:.1f}" y1="{y + 32:.1f}" x2="{x + width - 10:.1f}" y2="{y + 32:.1f}" stroke="{LIGHT}" stroke-width="0.8" />',
        f'<defs><clipPath id="{clip_id}">{svg_rect(map_x, map_y, map_w, map_h)}</clipPath></defs>',
        svg_rect(map_x, map_y, map_w, map_h, fill="#ffffff", stroke=LIGHT, **{
            "stroke-width": "0.6",
        }),
        f'<g clip-path="url(#{clip_id})">{"".join(basemap_images)}{"".join(rendered_layers)}</g>',
    ]
    return "\n".join(parts)


def render_legend() -> str:
    y = LEGEND_Y
    x = (WIDTH / 2) - 96

    parts = [
        f'<line x1="{FIG_LEFT:.1f}" y1="{y - 20:.1f}" x2="{WIDTH - FIG_LEFT:.1f}" y2="{y - 20:.1f}" stroke="{LIGHT}" stroke-width="0.8" />',
        svg_text(x, y, "α levels", fill=INK, **{
        "font-size": "12",
        "font-family": UI_FONT,
    })]

    x += 62
    for alpha_label, color in zip(("0.2", "0.4", "0.6"), ALPHA_COLORS):
        parts.append(svg_rect(x, y - 10, 14, 14, fill=color, stroke="#666666", **{
            "stroke-width": "0.3",
        }))
        parts.append(svg_text(x + 20, y + 1, alpha_label, fill=MUTED, **{
            "font-size": "11",
            "font-family": MONO_FONT,
        }))
        x += 58

    parts.append(svg_text(WIDTH - FIG_LEFT, y + 1, OSM_ATTRIBUTION, fill=MUTED, **{
        "font-size": "10",
        "font-family": UI_FONT,
        "text-anchor": "end",
    }))

    return "\n".join(parts)


def render_svg(analysis: dict) -> str:
    bounds = focus_bounds(analysis)
    basemap_tiles = build_osm_basemap(bounds)

    panel_y = FIG_TOP

    alpha_layers = lambda group: [
        (group["alpha_0_2"], ALPHA_COLORS[0], "0.46", "#3f8750", "0.40"),
        (group["alpha_0_4"], ALPHA_COLORS[1], "0.54", "#b58f04", "0.48"),
        (group["alpha_0_6"], ALPHA_COLORS[2], "0.62", "#962d2a", "0.56"),
    ]

    panel_a = render_alpha_object_panel(
        FIG_LEFT,
        panel_y,
        PANEL_W,
        PANEL_H,
        "(a) moderate",
        alpha_layers(analysis["moderate"]),
        basemap_tiles,
        bounds,
        "panel-a-clip",
    )
    panel_b = render_alpha_object_panel(
        FIG_LEFT + PANEL_W + COL_GAP,
        panel_y,
        PANEL_W,
        PANEL_H,
        "(b) high",
        alpha_layers(analysis["high"]),
        basemap_tiles,
        bounds,
        "panel-b-clip",
    )
    panel_c = render_alpha_object_panel(
        FIG_LEFT + (2 * (PANEL_W + COL_GAP)),
        panel_y,
        PANEL_W,
        PANEL_H,
        "(c) moderate ∩ high",
        alpha_layers(analysis["transition"]),
        basemap_tiles,
        bounds,
        "panel-c-clip",
    )

    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{HEIGHT}" viewBox="0 0 {WIDTH} {HEIGHT}">
  <rect width="100%" height="100%" fill="{BG}" />
  {svg_text(FIG_LEFT, 42, 'Stored fuzzyregion objects and their intersection', fill=INK, **{'font-size': '28', 'font-weight': '700', 'font-family': TITLE_FONT})}
  <line x1="{FIG_LEFT:.1f}" y1="60.0" x2="{WIDTH - FIG_LEFT:.1f}" y2="60.0" stroke="{LIGHT}" stroke-width="0.9" />
  {panel_a}
  {panel_b}
  {panel_c}
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
