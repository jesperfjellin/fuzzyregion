\set ON_ERROR_STOP on

DO $$
DECLARE
  fr fuzzyregion;
  inner_region fuzzyregion;
  touching_left fuzzyregion;
  touching_right fuzzyregion;
  unnormalized fuzzyregion;
  normalized fuzzyregion;
  concentrated fuzzyregion;
  dilated fuzzyregion;
  union_result fuzzyregion;
  intersection_result fuzzyregion;
  difference_result fuzzyregion;
  touching_intersection fuzzyregion;
  levels_json jsonb;
  export_json jsonb;
  invalid_literal text;
BEGIN
  fr := fuzzyregion_from_geoms(
    ARRAY[1.0, 0.5],
    ARRAY[
      ST_SetSRID(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'), 4326),
      ST_SetSRID(ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))'), 4326)
    ]
  );

  IF fuzzyregion_srid(fr) <> 4326 THEN
    RAISE EXCEPTION 'unexpected SRID: %', fuzzyregion_srid(fr);
  END IF;

  IF NOT fuzzyregion_is_valid(fr) THEN
    RAISE EXCEPTION 'valid fuzzyregion unexpectedly failed validation';
  END IF;

  IF COALESCE(array_length(fuzzyregion_validate(fr), 1), 0) <> 0 THEN
    RAISE EXCEPTION 'valid fuzzyregion returned validation errors: %', fuzzyregion_validate(fr);
  END IF;

  IF fuzzyregion_num_levels(fr) <> 2 THEN
    RAISE EXCEPTION 'unexpected level count: %', fuzzyregion_num_levels(fr);
  END IF;

  IF fuzzyregion_min_alpha(fr) <> 0.5 THEN
    RAISE EXCEPTION 'unexpected minimum alpha: %', fuzzyregion_min_alpha(fr);
  END IF;

  IF fuzzyregion_max_alpha(fr) <> 1.0 THEN
    RAISE EXCEPTION 'unexpected maximum alpha: %', fuzzyregion_max_alpha(fr);
  END IF;

  levels_json := fuzzyregion_levels(fr);

  IF levels_json->>'srid' <> '4326' THEN
    RAISE EXCEPTION 'levels JSON did not preserve SRID metadata: %', levels_json;
  END IF;

  IF jsonb_array_length(levels_json->'levels') <> 2 THEN
    RAISE EXCEPTION 'levels JSON did not return two levels: %', levels_json;
  END IF;

  IF ((levels_json->'levels'->0->>'alpha')::double precision) <> 1.0 THEN
    RAISE EXCEPTION 'levels JSON did not preserve highest alpha ordering: %', levels_json;
  END IF;

  export_json := fuzzyregion_to_jsonb(fr);

  IF export_json->>'max_alpha' <> '1.0' THEN
    RAISE EXCEPTION 'human-readable JSON did not expose max_alpha: %', export_json;
  END IF;

  IF (export_json->'levels'->0->>'geometry_ewkt') NOT LIKE 'SRID=4326;MULTIPOLYGON%' THEN
    RAISE EXCEPTION 'human-readable JSON did not include EWKT geometry export: %', export_json;
  END IF;

  IF fuzzyregion_to_text(fr) NOT LIKE 'v1;srid=4326;levels=[%' THEN
    RAISE EXCEPTION 'debug text export did not match the expected envelope';
  END IF;

  IF NOT ST_Equals(
    fuzzyregion_support(fr),
    ST_Multi(ST_SetSRID(ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))'), 4326))
  ) THEN
    RAISE EXCEPTION 'support projection did not match the expected geometry';
  END IF;

  IF NOT ST_Equals(
    fuzzyregion_core(fr),
    ST_Multi(ST_SetSRID(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'), 4326))
  ) THEN
    RAISE EXCEPTION 'core projection did not match the expected geometry';
  END IF;

  IF NOT ST_Equals(
    fuzzyregion_alpha_cut(fr, 0.75),
    ST_Multi(ST_SetSRID(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'), 4326))
  ) THEN
    RAISE EXCEPTION 'alpha-cut projection did not match the expected geometry';
  END IF;

  IF fuzzyregion_membership_at(
    fr,
    ST_SetSRID(ST_Point(0.5, 0.5), 4326)
  ) <> 1.0 THEN
    RAISE EXCEPTION 'membership_at for core point did not return 1.0';
  END IF;

  IF fuzzyregion_membership_at(
    fr,
    ST_SetSRID(ST_Point(1.5, 1.5), 4326)
  ) <> 0.5 THEN
    RAISE EXCEPTION 'membership_at for support-only point did not return 0.5';
  END IF;

  IF fuzzyregion_membership_at(
    fr,
    ST_SetSRID(ST_Point(3, 3), 4326)
  ) <> 0.0 THEN
    RAISE EXCEPTION 'membership_at for external point did not return 0.0';
  END IF;

  IF NOT ST_Equals(
    fuzzyregion_bbox(fr),
    ST_SetSRID(ST_GeomFromText('POLYGON((0 0,0 2,2 2,2 0,0 0))'), 4326)
  ) THEN
    RAISE EXCEPTION 'bbox projection did not match the expected envelope';
  END IF;

  IF fuzzyregion_area_at(fr, 0.75) <> 1.0 THEN
    RAISE EXCEPTION 'area_at for the higher alpha-cut did not return 1.0';
  END IF;

  IF fuzzyregion_area_at(fr, 0.0) <> 4.0 THEN
    RAISE EXCEPTION 'area_at for support did not return 4.0';
  END IF;

  inner_region := fuzzyregion_from_geoms(
    ARRAY[1.0, 0.5],
    ARRAY[
      ST_SetSRID(ST_GeomFromText('POLYGON((0.25 0.25,0.75 0.25,0.75 0.75,0.25 0.75,0.25 0.25))'), 4326),
      ST_SetSRID(ST_GeomFromText('POLYGON((0.25 0.25,1.5 0.25,1.5 1.5,0.25 1.5,0.25 0.25))'), 4326)
    ]
  );

  touching_left := fuzzyregion_from_geoms(
    ARRAY[1.0],
    ARRAY[
      ST_SetSRID(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'), 4326)
    ]
  );

  touching_right := fuzzyregion_from_geoms(
    ARRAY[1.0],
    ARRAY[
      ST_SetSRID(ST_GeomFromText('POLYGON((1 1,2 1,2 2,1 2,1 1))'), 4326)
    ]
  );

  unnormalized := fuzzyregion_from_geoms(
    ARRAY[0.8, 0.4],
    ARRAY[
      ST_SetSRID(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'), 4326),
      ST_SetSRID(ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))'), 4326)
    ]
  );

  normalized := fuzzyregion_normalize(unnormalized);
  concentrated := fuzzyregion_concentrate(unnormalized, 2.0);
  dilated := fuzzyregion_dilate_membership(unnormalized, 2.0);

  union_result := fuzzyregion_union(fr, inner_region);
  intersection_result := fuzzyregion_intersection(fr, inner_region);
  difference_result := fuzzyregion_difference(fr, inner_region);
  touching_intersection := fuzzyregion_intersection(touching_left, touching_right);

  IF NOT ST_Equals(
    fuzzyregion_support(union_result),
    fuzzyregion_support(fr)
  ) THEN
    RAISE EXCEPTION 'union support did not preserve the containing operand';
  END IF;

  IF NOT ST_Equals(
    fuzzyregion_core(intersection_result),
    fuzzyregion_core(inner_region)
  ) THEN
    RAISE EXCEPTION 'intersection core did not preserve the nested operand';
  END IF;

  IF fuzzyregion_membership_at(
    difference_result,
    ST_SetSRID(ST_Point(0.1, 0.1), 4326)
  ) <> 1.0 THEN
    RAISE EXCEPTION 'difference membership_at did not preserve the unaffected core area';
  END IF;

  IF fuzzyregion_membership_at(
    difference_result,
    ST_SetSRID(ST_Point(0.6, 0.6), 4326)
  ) <> 0.0 THEN
    RAISE EXCEPTION 'difference membership_at did not remove the nested core area';
  END IF;

  IF NOT ST_IsEmpty(fuzzyregion_support(touching_intersection)) THEN
    RAISE EXCEPTION 'intersection should drop boundary-only overlap and return an empty fuzzyregion';
  END IF;

  IF fuzzyregion_max_alpha(normalized) <> 1.0 OR fuzzyregion_min_alpha(normalized) <> 0.5 THEN
    RAISE EXCEPTION 'normalize did not rescale alphas as expected';
  END IF;

  IF abs(
    fuzzyregion_membership_at(concentrated, ST_SetSRID(ST_Point(1.5, 1.5), 4326)) - 0.16
  ) > 1e-12 THEN
    RAISE EXCEPTION 'concentrate did not square the support-only alpha as expected';
  END IF;

  IF abs(
    fuzzyregion_membership_at(dilated, ST_SetSRID(ST_Point(1.5, 1.5), 4326)) - sqrt(0.4)
  ) > 1e-12 THEN
    RAISE EXCEPTION 'dilate_membership did not soften the support-only alpha as expected';
  END IF;

  invalid_literal := format(
    'v1;srid=4326;levels=[1:%s,0.5:%s]',
    encode(
      ST_AsEWKB(ST_SetSRID(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'), 4326)),
      'hex'
    ),
    encode(
      ST_AsEWKB(ST_SetSRID(ST_GeomFromText('POLYGON((10 10,11 10,11 11,10 11,10 10))'), 4326)),
      'hex'
    )
  );

  BEGIN
    PERFORM invalid_literal::fuzzyregion;
    RAISE EXCEPTION 'invalid low-level fuzzyregion literal should have been rejected';
  EXCEPTION
    WHEN OTHERS THEN
      NULL;
  END;

  BEGIN
    PERFORM fuzzyregion_concentrate(fr, 1.0);
    RAISE EXCEPTION 'invalid concentrate power should have been rejected';
  EXCEPTION
    WHEN OTHERS THEN
      NULL;
  END;
END
$$;
