use criterion::{criterion_group, criterion_main, Criterion};
use skrifa::instance::LocationRef;
use skrifa::FontRef;
use sleipnir::draw_icon::{DrawIcon, DrawOptions, DrawType, ViewBoxMode};
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
            DrawType::Svg,
        );
        b.iter(|| black_box(font.draw_icon(black_box(&options))))
    })
    .bench_function("icon2svg/color", |b| {
        let font = FontRef::new(NOTO_EMOJI_FONT_BYTES).unwrap();
        let options = DrawOptions::new(
            IconIdentifier::Codepoint('🥳' as u32),
            24.0,
            LocationRef::default(),
            SvgPathStyle::Unchanged(2),
            DrawType::Svg,
        );
        b.iter(|| black_box(font.draw_icon(black_box(&options))))
    });
}

fn bench_icon2compose(c: &mut Criterion) {
    c.bench_function("icon2compose", |b| {
        let font = FontRef::new(ICON_FONT_BYTES).unwrap();
        let options = DrawOptions {
            viewbox_mode: ViewBoxMode::UseHeight,
            ..DrawOptions::new(
                IconIdentifier::Codepoint(57688), // MAIL
                24.0,
                LocationRef::default(),
                SvgPathStyle::Compact(2),
                DrawType::ComposeImageVector {
                    variable_name: "Mail",
                    package: "com.example.test",
                },
            )
        };
        b.iter(|| black_box(font.draw_icon(black_box(&options))))
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

criterion_group!(benches, bench_icon2svg, bench_icon2compose, bench_text2png);
criterion_main!(benches);
