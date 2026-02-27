use criterion::{criterion_group, criterion_main, Criterion};
use skrifa::instance::LocationRef;
use skrifa::FontRef;
use sleipnir::draw_glyph::DrawOptions;
use sleipnir::icon2kt::draw_kt;
use sleipnir::icon2svg::draw_icon;
use sleipnir::iconid::IconIdentifier;
use sleipnir::pathstyle::SvgPathStyle;
use sleipnir::text2png::{text2png, Text2PngOptions};
use std::hint::black_box;

const ICON_FONT_BYTES: &[u8] = include_bytes!("../resources/testdata/vf[FILL,GRAD,opsz,wght].ttf");
const CAVEAT_FONT_BYTES: &[u8] = include_bytes!("../resources/testdata/caveat.ttf");
const NOTO_EMOJI_FONT_BYTES: &[u8] = include_bytes!("../resources/testdata/NotoColorEmoji.ttf");

fn bench_icon2svg(c: &mut Criterion) {
    c.bench_function("icon2svg/simple", |b| {
        let font = FontRef::new(ICON_FONT_BYTES).unwrap();
        let options = DrawOptions::new(
            IconIdentifier::Codepoint(57688), // MAIL
            24.0,
            LocationRef::default(),
            SvgPathStyle::Unchanged(2),
        );
        b.iter(|| black_box(draw_icon(black_box(&font), black_box(&options))))
    })
    .bench_function("icon2svg/color", |b| {
        let font = FontRef::new(NOTO_EMOJI_FONT_BYTES).unwrap();
        let options = DrawOptions::new(
            IconIdentifier::Codepoint('🥳' as u32),
            24.0,
            LocationRef::default(),
            SvgPathStyle::Unchanged(2),
        );
        b.iter(|| black_box(draw_icon(black_box(&font), black_box(&options))))
    });
}

fn bench_icon2kt(c: &mut Criterion) {
    c.bench_function("icon2kt", |b| {
        let font = FontRef::new(ICON_FONT_BYTES).unwrap();
        let options = DrawOptions {
            kt_variable_name: "Mail",
            use_width_height_for_viewbox: true,
            ..DrawOptions::new(
                IconIdentifier::Codepoint(57688), // MAIL
                24.0,
                LocationRef::default(),
                SvgPathStyle::Compact(2),
            )
        };
        b.iter(|| {
            black_box(draw_kt(
                black_box(&font),
                black_box(&options),
                black_box("com.example.test"),
            ))
        })
    });
}

fn bench_text2png(c: &mut Criterion) {
    c.bench_function("text2png/simple", |b| {
        let options = Text2PngOptions::new(CAVEAT_FONT_BYTES, 24.0);
        b.iter(|| black_box(text2png(black_box("hello world"), black_box(&options))))
    })
    .bench_function("text2png/color", |b| {
        let options = Text2PngOptions::new(NOTO_EMOJI_FONT_BYTES, 24.0);
        b.iter(|| black_box(text2png(black_box("🥳"), black_box(&options))))
    });
}

criterion_group!(benches, bench_icon2svg, bench_icon2kt, bench_text2png);
criterion_main!(benches);
