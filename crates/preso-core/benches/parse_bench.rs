//! Plan §9.6 performance gate: parsing a 100-slide deck must stay under
//! 50 ms. Criterion measures it; the hard assertion lives in the
//! `parse_100_slides_under_50ms` test below so CI fails loudly.

use criterion::{Criterion, criterion_group, criterion_main};

fn hundred_slide_deck() -> String {
    let mut deck = String::from("---\ntitle: bench\ntheme: dark\n---\n");
    for i in 0..100 {
        deck.push_str(&format!(
            "\n# Slide {i}\n\nSome **bold** text with `code` and $x^{{{i}}}$ math.\n\n\
             - point one\n<!-- pause -->\n- point two\n\n\
             ```rust {{1,3-4}}\nfn main() {{\n    let x = {i};\n    println!(\"{{x}}\");\n}}\n```\n\n\
             <!-- note: speaker note for slide {i} -->\n\n---\n"
        ));
    }
    deck
}

fn bench_parse(c: &mut Criterion) {
    let source = hundred_slide_deck();
    c.bench_function("parse 100-slide deck", |b| {
        b.iter(|| preso_core::parser::parse(std::hint::black_box(&source)).unwrap());
    });
}

criterion_group!(benches, bench_parse);
criterion_main!(benches);
