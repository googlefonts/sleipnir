use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use resvg::{
    tiny_skia::{Pixmap, Transform},
    usvg::{Options, Tree},
};
use skrifa::{prelude::LocationRef, FontRef};
use sleipnir::{
    draw_glyph::DrawOptions, icon2png::icon2png, icon2svg::draw_icon, iconid::IconIdentifier,
    pathstyle::SvgPathStyle,
};

static ICON_FONT: &[u8] = include_bytes!("../resources/testdata/vf[FILL,GRAD,opsz,wght].ttf");
static NOTO_EMOJI_FONT: &[u8] = include_bytes!("../resources/testdata/NotoColorEmoji.ttf");

fn icon2png_resvg(svg: &str, size: u32) -> Vec<u8> {
    let tree = Tree::from_str(svg, &Options::default()).unwrap();
    let svg_size = tree.size();
    let scale = size as f32 / svg_size.width().max(svg_size.height());
    let mut pixmap = Pixmap::new(size, size).unwrap();
    resvg::render(
        &tree,
        Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap.encode_png().unwrap()
}

fn bench_icon2svg(c: &mut Criterion) {
    c.bench_function("draw_icon/simple", |b| {
        let font = FontRef::new(ICON_FONT).unwrap();
        let options = DrawOptions::new(
            IconIdentifier::Codepoint(57688), // mail
            24.0,
            LocationRef::default(),
            SvgPathStyle::Compact(2),
        );
        b.iter(|| draw_icon(black_box(&font), black_box(&options)).unwrap())
    })
    .bench_function("draw_icon/color", |b| {
        let font = FontRef::new(NOTO_EMOJI_FONT).unwrap();
        let options = DrawOptions::new(
            IconIdentifier::Codepoint('🥳' as u32),
            24.0,
            LocationRef::default(),
            SvgPathStyle::Unchanged(2),
        );
        b.iter(|| draw_icon(black_box(&font), black_box(&options)).unwrap())
    });
}

fn bench_icon2png(c: &mut Criterion) {
    c.bench_function("icon2png/simple", |b| {
        let font = FontRef::new(ICON_FONT).unwrap();
        let options = DrawOptions::new(
            IconIdentifier::Codepoint(57688), // mail
            24.0,
            LocationRef::default(),
            SvgPathStyle::Compact(2),
        );
        b.iter(|| icon2png(black_box(&font), black_box(&options)).unwrap())
    })
    .bench_function("icon2png/color", |b| {
        let font = FontRef::new(NOTO_EMOJI_FONT).unwrap();
        let options = DrawOptions::new(
            IconIdentifier::Codepoint('🥳' as u32),
            24.0,
            LocationRef::default(),
            SvgPathStyle::Unchanged(2),
        );
        b.iter(|| icon2png(black_box(&font), black_box(&options)).unwrap())
    });
}

fn bench_icon2png_resvg(c: &mut Criterion) {
    c.bench_function("icon2png_resvg/simple", |b| {
        let font = FontRef::new(ICON_FONT).unwrap();
        let options = DrawOptions::new(
            IconIdentifier::Codepoint(57688), // mail
            24.0,
            LocationRef::default(),
            SvgPathStyle::Compact(2),
        );
        let svg = draw_icon(&font, &options).unwrap();
        b.iter(|| icon2png_resvg(black_box(&svg), black_box(24)))
    })
    .bench_function("icon2png_resvg/color", |b| {
        let font = FontRef::new(NOTO_EMOJI_FONT).unwrap();
        let options = DrawOptions::new(
            IconIdentifier::Codepoint('🥳' as u32),
            24.0,
            LocationRef::default(),
            SvgPathStyle::Unchanged(2),
        );
        let svg = draw_icon(&font, &options).unwrap();
        b.iter(|| icon2png_resvg(black_box(&svg), black_box(24)))
    });
}

criterion_group!(
    benches,
    bench_icon2svg,
    bench_icon2png,
    bench_icon2png_resvg
);
criterion_main!(benches);
