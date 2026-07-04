//! Plan §9.6 performance gate, enforced in CI: a 100-slide deck parses in
//! under 50 ms. The expected time is well under 5 ms, so the margin
//! absorbs noisy CI runners.

#[test]
fn parse_100_slides_under_50ms() {
    let mut deck = String::from("---\ntitle: bench\ntheme: dark\n---\n");
    for i in 0..100 {
        deck.push_str(&format!(
            "\n# Slide {i}\n\nSome **bold** text with `code` and $x^{{{i}}}$ math.\n\n\
             - point one\n<!-- pause -->\n- point two\n\n\
             ```rust {{1,3-4}}\nfn main() {{\n    let x = {i};\n    println!(\"{{x}}\");\n}}\n```\n\n\
             <!-- note: speaker note for slide {i} -->\n\n---\n"
        ));
    }

    // Warm up, then measure the median of several runs.
    let _ = preso_core::parser::parse(&deck).unwrap();
    let mut timings: Vec<std::time::Duration> = (0..9)
        .map(|_| {
            let start = std::time::Instant::now();
            let parsed = preso_core::parser::parse(&deck).unwrap();
            let elapsed = start.elapsed();
            assert_eq!(parsed.slides.len(), 100);
            elapsed
        })
        .collect();
    timings.sort();
    let median = timings[timings.len() / 2];
    assert!(
        median < std::time::Duration::from_millis(50),
        "100-slide parse took {median:?} (budget: 50ms)"
    );
}
