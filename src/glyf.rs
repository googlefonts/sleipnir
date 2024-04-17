use skrifa::raw::{
    tables::{
        glyf::{
            self, Anchor, CompositeGlyph, CompositeGlyphFlags, Glyf, Glyph, PointFlags,
            PointMarker, SimpleGlyph, ToPathStyle,
        },
        gvar::{GlyphDelta, Gvar},
        hmtx::Hmtx,
        loca::Loca,
        variations::TupleVariation,
    },
    types::{F2Dot14, GlyphId, Pen, Point},
    FontRef, ReadError, TableProvider,
};

const PHANTOM_POINT_COUNT: usize = 4;

pub fn draw(
    font: &FontRef,
    coords: &[F2Dot14],
    glyph_id: GlyphId,
    pen: &mut impl Pen,
) -> Result<(), ReadError> {
    let scaler = Scaler::new(font)?;
    let mut outline = Outline::default();
    scaler.draw(glyph_id, coords, &mut outline)?;
    glyf::to_path(
        &outline.points,
        &outline.flags,
        &outline.contours,
        ToPathStyle::HarfBuzz,
        pen,
    )
    .map_err(|_| ReadError::MalformedData("path conversion failed?"))?;
    Ok(())
}

#[derive(Clone, Default, Debug)]
struct Outline {
    pub points: Vec<Point<f32>>,
    pub contours: Vec<u16>,
    pub flags: Vec<PointFlags>,
    pub phantom: [Point<f32>; 4],
}

impl Outline {
    fn clear(&mut self) {
        self.points.clear();
        self.contours.clear();
        self.flags.clear();
        self.phantom = Default::default();
    }
}

#[derive(Clone)]
struct Scaler<'a> {
    pub glyf: Glyf<'a>,
    pub loca: Loca<'a>,
    pub gvar: Option<Gvar<'a>>,
    pub hmtx: Hmtx<'a>,
}

impl<'a> Scaler<'a> {
    fn new(font: &FontRef<'a>) -> Result<Self, ReadError> {
        let glyf = font.glyf()?;
        let loca = font.loca(None)?;
        let gvar = font.gvar().ok();
        let hmtx = font.hmtx()?;
        Ok(Self {
            glyf,
            loca,
            gvar,
            hmtx,
        })
    }

    fn draw(
        &self,
        glyph_id: GlyphId,
        coords: &'a [F2Dot14],
        outline: &mut Outline,
    ) -> Result<(), ReadError> {
        outline.clear();
        let glyph = self.loca.get_glyf(glyph_id, &self.glyf)?;
        self.read_glyph(coords, glyph_id, &glyph, outline)?;
        let x_shift = outline.phantom[0].x;
        if x_shift != 0.0 {
            for point in &mut outline.points {
                point.x -= x_shift;
            }
        }
        Ok(())
    }

    fn read_glyph(
        &self,
        coords: &'a [F2Dot14],
        glyph_id: GlyphId,
        glyph: &Option<Glyph>,
        outline: &mut Outline,
    ) -> Result<(), ReadError> {
        let Some(glyph) = glyph else {
            return Ok(());
        };
        outline.phantom = self.compute_phantom(glyph_id, glyph);
        match glyph {
            Glyph::Simple(simple) => self.read_simple(coords, glyph_id, simple, outline),
            Glyph::Composite(composite) => {
                self.read_composite(coords, glyph_id, composite, outline)
            }
        }
    }

    fn read_simple(
        &self,
        coords: &'a [F2Dot14],
        glyph_id: GlyphId,
        glyph: &SimpleGlyph,
        outline: &mut Outline,
    ) -> Result<(), ReadError> {
        let point_start = outline.points.len();
        let point_count = glyph.num_points() + PHANTOM_POINT_COUNT;
        let point_end = point_start + point_count - PHANTOM_POINT_COUNT;
        outline
            .points
            .resize(point_start + point_count, Default::default());
        outline
            .flags
            .resize(point_start + point_count, PointFlags::default());
        let contour_start = outline.contours.len();
        let end_pts = glyph.end_pts_of_contours();
        outline.contours.extend(end_pts.iter().map(|x| x.get()));
        let contours = &mut outline.contours[contour_start..];
        let points = &mut outline.points[point_start..];
        let phantom_start = point_count - PHANTOM_POINT_COUNT;
        glyph.read_points_fast(
            &mut points[..phantom_start],
            &mut outline.flags[point_start..point_end],
        )?;
        for (point, phantom) in points[phantom_start..].iter_mut().zip(&outline.phantom) {
            *point = *phantom;
        }
        let flags = &mut outline.flags[point_start..];
        if let (true, Some(gvar)) = (!coords.is_empty(), self.gvar.as_ref()) {
            let mut tuple_scratch = vec![Default::default(); point_count];
            let mut adjusted = vec![Default::default(); point_count];
            let glyph = SimpleOutline {
                points: &mut points[..],
                flags: &mut flags[..],
                contours,
            };
            if apply_simple_deltas(
                gvar,
                glyph_id,
                coords,
                glyph,
                &mut tuple_scratch,
                &mut adjusted,
            )
            .is_ok()
            {
                points.copy_from_slice(&adjusted);
            }
        }
        if point_start != 0 {
            for contour in contours {
                *contour += point_start as u16;
            }
        }
        for (point, phantom) in points[phantom_start..].iter().zip(&mut outline.phantom) {
            *phantom = *point;
        }
        outline
            .points
            .truncate(outline.points.len() - PHANTOM_POINT_COUNT);
        outline
            .flags
            .truncate(outline.flags.len() - PHANTOM_POINT_COUNT);
        Ok(())
    }

    fn read_composite(
        &self,
        coords: &'a [F2Dot14],
        glyph_id: GlyphId,
        glyph: &CompositeGlyph,
        outline: &mut Outline,
    ) -> Result<(), ReadError> {
        let mut deltas = vec![];
        if let (true, Some(gvar)) = (!coords.is_empty(), self.gvar.as_ref()) {
            let count = glyph.components().count() + PHANTOM_POINT_COUNT;
            deltas.resize(count, Default::default());
            let _ = compute_composite_deltas(gvar, glyph_id, coords, &mut deltas);
            for (phantom, delta) in outline
                .phantom
                .iter_mut()
                .zip(&deltas[count - PHANTOM_POINT_COUNT..])
            {
                *phantom += *delta;
            }
        }
        for (i, component) in glyph.components().enumerate() {
            let phantom = outline.phantom;
            let component_start = outline.points.len();
            let component_glyph = self.loca.get_glyf(component.glyph, &self.glyf)?;
            self.read_glyph(coords, component.glyph, &component_glyph, outline)?;
            let component_end = outline.points.len();
            if !component
                .flags
                .contains(CompositeGlyphFlags::USE_MY_METRICS)
            {
                outline.phantom = phantom;
            }
            let [xx, yx, xy, yy] = if component.flags.intersects(
                CompositeGlyphFlags::WE_HAVE_A_SCALE
                    | CompositeGlyphFlags::WE_HAVE_AN_X_AND_Y_SCALE
                    | CompositeGlyphFlags::WE_HAVE_A_TWO_BY_TWO,
            ) {
                let xform = &component.transform;
                [xform.xx, xform.yx, xform.xy, xform.yy].map(|x| x.to_f32())
            } else {
                [1.0, 0.0, 0.0, 1.0]
            };
            let offset = match component.anchor {
                Anchor::Offset { x, y } => {
                    Point::new(x as f32, y as f32) + deltas.get(i).copied().unwrap_or_default()
                }
                Anchor::Point { .. } => Point::default(),
            };
            let points = &mut outline.points[component_start..component_end];
            if component.flags
                & (CompositeGlyphFlags::SCALED_COMPONENT_OFFSET
                    | CompositeGlyphFlags::UNSCALED_COMPONENT_OFFSET)
                == CompositeGlyphFlags::SCALED_COMPONENT_OFFSET
            {
                for point in points.iter_mut() {
                    let trans = *point + offset;
                    point.x = trans.x * xx + trans.y * xy;
                    point.y = trans.y * yx + trans.y * yy;
                }
            } else {
                for point in points.iter_mut() {
                    let p = *point;
                    point.x = p.x * xx + p.y * xy + offset.x;
                    point.y = p.y * yx + p.y * yy + offset.y;
                }
            }
        }
        Ok(())
    }
}

impl Scaler<'_> {
    fn advance_width(&self, gid: GlyphId) -> i32 {
        let default_advance = self
            .hmtx
            .h_metrics()
            .last()
            .map(|metric| metric.advance())
            .unwrap_or(0);
        self.hmtx
            .h_metrics()
            .get(gid.to_u16() as usize)
            .map(|metric| metric.advance())
            .unwrap_or(default_advance) as i32
    }

    fn lsb(&self, gid: GlyphId) -> i32 {
        let gid_index = gid.to_u16() as usize;
        self.hmtx
            .h_metrics()
            .get(gid_index)
            .map(|metric| metric.side_bearing())
            .unwrap_or_else(|| {
                self.hmtx
                    .left_side_bearings()
                    .get(gid_index.saturating_sub(self.hmtx.h_metrics().len()))
                    .map(|lsb| lsb.get())
                    .unwrap_or(0)
            }) as i32
    }

    fn compute_phantom(&self, glyph_id: GlyphId, glyph: &Glyph) -> [Point<f32>; 4] {
        let left = glyph.x_min() as f32 - self.lsb(glyph_id) as f32;
        let right = left + self.advance_width(glyph_id) as f32;
        [
            Point::new(left, 0.0),
            Point::new(right, 0.0),
            Point::default(),
            Point::default(),
        ]
    }
}

/// Compute a set of deltas for the component offsets of a composite glyph.
///
/// Interpolation is meaningless for component offsets so this is a
/// specialized function that skips the expensive bits.
fn compute_composite_deltas(
    gvar: &Gvar,
    glyph_id: GlyphId,
    coords: &[F2Dot14],
    deltas: &mut [Point<f32>],
) -> Result<(), ReadError> {
    compute_deltas_for_glyph(gvar, glyph_id, coords, deltas, |scalar, tuple, deltas| {
        for tuple_delta in tuple.deltas() {
            let ix = tuple_delta.position as usize;
            if let Some(delta) = deltas.get_mut(ix) {
                delta.x += tuple_delta.x_delta as f32 * scalar;
                delta.y += tuple_delta.y_delta as f32 * scalar;
            }
        }
        Ok(())
    })?;
    Ok(())
}

struct SimpleOutline<'a> {
    pub points: &'a [Point<f32>],
    pub flags: &'a mut [PointFlags],
    pub contours: &'a [u16],
}

/// Applies a set of deltas to the points in a simple glyph.
///
/// This function will use interpolation to infer missing deltas for tuples
/// that contain sparse sets. The `tuple_scratch` buffer is temporary storage
/// used for this and the length must be >= glyph.points.len().
///
/// The `adjusted_points` slice will contain the full points with deltas
/// applied.
fn apply_simple_deltas(
    gvar: &Gvar,
    glyph_id: GlyphId,
    coords: &[F2Dot14],
    glyph: SimpleOutline,
    tuple_scratch: &mut [Point<f32>],
    adjusted_points: &mut [Point<f32>],
) -> Result<(), ReadError> {
    if tuple_scratch.len() < glyph.points.len() || glyph.points.len() < PHANTOM_POINT_COUNT {
        return Err(ReadError::InvalidArrayLen);
    }
    adjusted_points.copy_from_slice(glyph.points);
    if gvar.glyph_variation_data(glyph_id).is_err() {
        // Empty variation data for a glyph is not an error.
        return Ok(());
    };
    let SimpleOutline {
        points,
        flags,
        contours,
    } = glyph;
    compute_deltas_for_glyph(
        gvar,
        glyph_id,
        coords,
        adjusted_points,
        |scalar, tuple, adjusted_points| {
            // Infer missing deltas by interpolation.
            // Prepare our working buffer by copying the points
            // and clearing the HAS_DELTA flags.
            for (flag, scratch) in flags.iter_mut().zip(&mut tuple_scratch[..]) {
                *scratch = Default::default();
                flag.clear_marker(PointMarker::HAS_DELTA);
            }
            for tuple_delta in tuple.deltas() {
                let ix = tuple_delta.position as usize;
                if let (Some(flag), Some(scratch)) = (flags.get_mut(ix), tuple_scratch.get_mut(ix))
                {
                    flag.set_marker(PointMarker::HAS_DELTA);
                    scratch.x += tuple_delta.x_delta as f32 * scalar;
                    scratch.y += tuple_delta.y_delta as f32 * scalar;
                }
            }
            interpolate_deltas(points, flags, contours, &mut tuple_scratch[..])
                .ok_or(ReadError::OutOfBounds)?;
            for (adjusted, scratch) in adjusted_points.iter_mut().zip(tuple_scratch.iter()) {
                *adjusted += *scratch;
            }
            Ok(())
        },
    )?;
    Ok(())
}

/// The common parts of simple and complex glyph processing
fn compute_deltas_for_glyph(
    gvar: &Gvar,
    glyph_id: GlyphId,
    coords: &[F2Dot14],
    deltas: &mut [Point<f32>],
    mut apply_tuple_missing_deltas_fn: impl FnMut(
        f32,
        TupleVariation<GlyphDelta>,
        &mut [Point<f32>],
    ) -> Result<(), ReadError>,
) -> Result<(), ReadError> {
    // for delta in deltas.iter_mut() {
    //     *delta = Default::default();
    // }
    let Ok(var_data) = gvar.glyph_variation_data(glyph_id) else {
        // Empty variation data for a glyph is not an error.
        return Ok(());
    };
    let active_tuples = var_data.tuples().filter_map(|tuple| {
        let scalar = tuple.compute_scalar_f32(coords)?;
        Some((tuple, scalar))
    });
    for (tuple, scalar) in active_tuples {
        // Fast path: tuple contains all points, we can simply accumulate
        // the deltas directly.
        if tuple.has_deltas_for_all_points() {
            for (delta, tuple_delta) in deltas.iter_mut().zip(tuple.deltas()) {
                delta.x += tuple_delta.x_delta as f32 * scalar;
                delta.y += tuple_delta.y_delta as f32 * scalar;
            }
        } else {
            // Slow path is, annoyingly, different for simple vs composite
            // so let the caller handle it
            apply_tuple_missing_deltas_fn(scalar, tuple, deltas)?;
        }
    }
    Ok(())
}

fn interpolate_deltas(
    points: &[Point<f32>],
    flags: &[PointFlags],
    contours: &[u16],
    deltas: &mut [Point<f32>],
) -> Option<()> {
    const DELTA: PointMarker = PointMarker::HAS_DELTA;
    let mut start_ix = 0;
    for &end_ix in contours {
        let end_ix = end_ix as usize;
        if end_ix < start_ix {
            return None;
        }
        let point_range = start_ix..end_ix + 1;
        let mut unref_count = flags
            .get(point_range.clone())?
            .iter()
            .filter(|flag| flag.has_marker(DELTA))
            .count();
        unref_count = (end_ix - start_ix + 1) - unref_count;
        if unref_count == 0 || unref_count > end_ix - start_ix {
            start_ix = end_ix + 1;
            continue;
        }
        let next_index = move |i: usize| {
            if i >= end_ix {
                start_ix
            } else {
                i + 1
            }
        };
        let mut j = start_ix;
        start_ix = end_ix + 1;
        'outer: loop {
            let mut i;
            loop {
                i = j;
                j = next_index(i);
                if flags[i].has_marker(DELTA) && !flags[j].has_marker(DELTA) {
                    break;
                }
            }
            j = i;
            let prev = i;
            loop {
                i = j;
                j = next_index(i);
                if !flags[i].has_marker(DELTA) && flags[j].has_marker(DELTA) {
                    break;
                }
            }
            let next = j;
            i = prev;
            loop {
                i = next_index(i);
                if i == next {
                    break;
                }
                macro_rules! interp_coord {
                    ($coord:ident) => {
                        let target_val = points.get(i)?.$coord;
                        let prev_val = points.get(prev)?.$coord;
                        let next_val = points.get(next)?.$coord;
                        let prev_delta = deltas.get(prev)?.$coord;
                        let next_delta = deltas.get(next)?.$coord;
                        let delta = if prev_val == next_val {
                            if prev_delta == next_delta {
                                prev_delta
                            } else {
                                0.0
                            }
                        } else if target_val <= prev_val.min(next_val) {
                            if prev_val < next_val {
                                prev_delta
                            } else {
                                next_delta
                            }
                        } else if target_val >= prev_val.max(next_val) {
                            if prev_val > next_val {
                                prev_delta
                            } else {
                                next_delta
                            }
                        } else {
                            let r = (target_val - prev_val) / (next_val - prev_val);
                            prev_delta + r * (next_delta - prev_delta)
                        };
                        deltas.get_mut(i)?.$coord = delta;
                    };
                }
                interp_coord!(x);
                interp_coord!(y);
                unref_count -= 1;
                if unref_count == 0 {
                    break 'outer;
                }
            }
        }
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_points(tuples: &[(i32, i32)]) -> Vec<Point<f32>> {
        tuples
            .iter()
            .map(|&(x, y)| Point::new(x as f32, y as f32))
            .collect()
    }

    fn make_flags(deltas: &[Point<f32>]) -> Vec<PointFlags> {
        deltas
            .iter()
            .map(|delta| {
                let mut flags = PointFlags::default();
                if delta.x != 0.0 || delta.y != 0.0 {
                    flags.set_marker(PointMarker::HAS_DELTA);
                }
                flags
            })
            .collect()
    }

    #[test]
    fn shift() {
        let points = make_points(&[(245, 630), (260, 700), (305, 680)]);
        // Single delta triggers a full contour shift.
        let mut deltas = make_points(&[(20, -10), (0, 0), (0, 0)]);
        let flags = make_flags(&deltas);
        interpolate_deltas(&points, &flags, &[2], &mut deltas).unwrap();
        let new_points = points
            .iter()
            .zip(&deltas)
            .map(|(p, d)| *p + *d)
            .collect::<Vec<_>>();
        let expected = &[
            Point::new(265, 620).map(|x| x as f32),
            Point::new(280, 690).map(|x| x as f32),
            Point::new(325, 670).map(|x| x as f32),
        ];
        assert_eq!(&new_points, expected);
    }

    #[test]
    fn interpolate() {
        // Test taken from the spec:
        // https://learn.microsoft.com/en-us/typography/opentype/spec/gvar#inferred-deltas-for-un-referenced-point-numbers
        // with a minor adjustment to account for the precision of our fixed point math.
        let points = make_points(&[(245, 630), (260, 700), (305, 680)]);
        let mut deltas = make_points(&[(28, -62), (0, 0), (-42, -57)]);
        let flags = make_flags(&deltas);
        interpolate_deltas(&points, &flags, &[2], &mut deltas).unwrap();
        let new_points = points
            .iter()
            .zip(&deltas)
            .map(|(p, d)| *p + *d)
            .collect::<Vec<_>>();
        assert_eq!(new_points[1], Point::new(260.0 + 10.5, 700.0 - 57.0));
    }
}
