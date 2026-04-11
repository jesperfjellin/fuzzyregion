#!/usr/bin/env bash

# Prepares the selected real-data fuzzyregion demo inputs for the Po Valley
# using a single source phenomenon: aggregated tree-cover density.
#
# The script:
# - clips the tree-cover raster to the study area
# - treats tree-cover values above 100 as nodata/background
# - aggregates tree cover to a 1 km mean grid
# - derives two overlapping fuzzy classes from that same source:
#   moderate_tree_cover and high_tree_cover
# - polygonizes alpha-cut layers for both classes
#
# This script is authoring-only. It exists to regenerate the shipped demo
# assets; it is not part of the normal extension workflow.
#
# Optional environment overrides:
# - FUZZYREGION_GDAL_IMAGE
# - FUZZYREGION_TREE_SOURCE
# - FUZZYREGION_OUTPUT_DIR
# - FUZZYREGION_LOG_PREFIX

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

gdal_image="${FUZZYREGION_GDAL_IMAGE:-ghcr.io/osgeo/gdal:ubuntu-small-latest}"
tree_source="${FUZZYREGION_TREE_SOURCE:-$repo_root/examples/data/tree_cover_density_2015/TCD_2015_100m_eu_03035_d04_full.tif}"
output_dir="${FUZZYREGION_OUTPUT_DIR:-$repo_root/examples/data/generated/po_valley_tree_cover_transition}"
log_prefix="${FUZZYREGION_LOG_PREFIX:-[fuzzyregion:data]}"

# Po Valley study area
bbox_3035_xmin="4063842"
bbox_3035_ymin="2326624"
bbox_3035_xmax="4596474"
bbox_3035_ymax="2627414"

alpha_levels=("0.6" "0.4" "0.2")

status() {
  echo "${log_prefix} $*"
}

require_file() {
  local path="$1"
  if [[ ! -f "$path" ]]; then
    echo "${log_prefix} Missing required input: $path" >&2
    exit 1
  fi
}

run_gdal() {
  docker run \
    --rm \
    --user "$(id -u):$(id -g)" \
    --volume "$repo_root:/workspace" \
    --workdir /workspace \
    "$gdal_image" \
    "$@"
}

alpha_slug() {
  echo "$1" | tr '.' '_'
}

polygonize_alpha_cut() {
  local source_raster="$1"
  local alpha="$2"
  local layer_name="$3"
  local out_geojson="$4"

  local mask_tif="${out_geojson%.geojson}.mask.tif"

  run_gdal gdal_calc.py \
    -A "$source_raster" \
    --outfile "$mask_tif" \
    --type Byte \
    --NoDataValue 0 \
    --calc "A>=${alpha}"

  run_gdal gdal_polygonize.py \
    "$mask_tif" \
    -mask "$mask_tif" \
    -f GeoJSON \
    "$out_geojson" \
    "$layer_name" \
    DN

  rm -f "$mask_tif"
}

require_file "$tree_source"

mkdir -p "$output_dir"

relative_output_dir="${output_dir#$repo_root/}"
relative_tree_source="${tree_source#$repo_root/}"

tree_clip="$relative_output_dir/tree_cover_po_valley_3035.tif"
tree_clip_clean="$relative_output_dir/tree_cover_po_valley_valid_3035.tif"
tree_1km_mean="$relative_output_dir/tree_cover_po_valley_1km_mean_3035.tif"
moderate_membership="$relative_output_dir/moderate_tree_cover_membership_po_valley_1km_3035.tif"
high_membership="$relative_output_dir/high_tree_cover_membership_po_valley_1km_3035.tif"

status "Using GDAL image: $gdal_image"
status "Output directory: $output_dir"

status "Clipping tree-cover raster to the Po Valley study area."
run_gdal gdalwarp \
  -overwrite \
  -te "$bbox_3035_xmin" "$bbox_3035_ymin" "$bbox_3035_xmax" "$bbox_3035_ymax" \
  -te_srs EPSG:3035 \
  -t_srs EPSG:3035 \
  -r near \
  "$relative_tree_source" \
  "$tree_clip"

status "Masking tree-cover values above 100 as nodata."
run_gdal gdal_calc.py \
  -A "$tree_clip" \
  --outfile "$tree_clip_clean" \
  --type Float32 \
  --NoDataValue -9999 \
  --calc "where(A>100,-9999,A)"

status "Aggregating tree cover to a 1 km mean grid."
run_gdal gdalwarp \
  -overwrite \
  -te "$bbox_3035_xmin" "$bbox_3035_ymin" "$bbox_3035_xmax" "$bbox_3035_ymax" \
  -te_srs EPSG:3035 \
  -t_srs EPSG:3035 \
  -tr 1000 1000 \
  -tap \
  -r average \
  -srcnodata -9999 \
  -dstnodata -9999 \
  "$tree_clip_clean" \
  "$tree_1km_mean"

status "Mapping aggregated tree cover into moderate-tree-cover membership."
run_gdal gdal_calc.py \
  -A "$tree_1km_mean" \
  --outfile "$moderate_membership" \
  --type Float32 \
  --NoDataValue -9999 \
  --calc "where(A==-9999,-9999,where(A<=20,0,where(A<40,(A-20)/20,where(A<=60,1,where(A<80,(80-A)/20,0)))))"

status "Mapping aggregated tree cover into high-tree-cover membership."
run_gdal gdal_calc.py \
  -A "$tree_1km_mean" \
  --outfile "$high_membership" \
  --type Float32 \
  --NoDataValue -9999 \
  --calc "where(A==-9999,-9999,where(A<=45,0,where(A<65,(A-45)/20,1)))"

for alpha in "${alpha_levels[@]}"; do
  slug="$(alpha_slug "$alpha")"

  status "Polygonizing moderate tree-cover alpha-cut ${alpha}."
  polygonize_alpha_cut \
    "$moderate_membership" \
    "$alpha" \
    "moderate_tree_alpha_${slug}" \
    "$relative_output_dir/moderate_tree_alpha_${slug}.geojson"

  status "Polygonizing high tree-cover alpha-cut ${alpha}."
  polygonize_alpha_cut \
    "$high_membership" \
    "$alpha" \
    "high_tree_alpha_${slug}" \
    "$relative_output_dir/high_tree_alpha_${slug}.geojson"
done

status "Prepared demo inputs:"
status "  Aggregated tree-cover raster:   $output_dir/tree_cover_po_valley_1km_mean_3035.tif"
status "  Moderate-class membership:      $output_dir/moderate_tree_cover_membership_po_valley_1km_3035.tif"
status "  High-class membership:          $output_dir/high_tree_cover_membership_po_valley_1km_3035.tif"
status "  Alpha-cut polygons:             $output_dir/*.geojson"
