use std::{
    hint::black_box,
    time::{Duration, Instant},
};

const LINES: usize = 200_000;
const VIEW: usize = 60;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 as usize
    }
}

fn millis(time: Duration) -> f64 {
    time.as_secs_f64() * 1000.0
}

fn main() {
    let mut lines: Vec<String> = (0..LINES)
        .map(|index| {
            format!("fn item_{index}(value: i64) -> i64 {{ value.wrapping_add({index}) }}")
        })
        .collect();
    let source = format!("{}\n", lines.join("\n"));
    let mut highlighter = markit::LineHighlighter::new("rust");
    let start = Instant::now();
    highlighter.set_text(&source);
    black_box(highlighter.tokens(0, VIEW));
    println!(
        "{LINES} lines, set_text + first {VIEW}-line viewport: {:.2} ms",
        millis(start.elapsed())
    );

    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut times = Vec::with_capacity(1_000);
    let mut line = 0;
    for _ in 0..1_000 {
        line = (line + 1 + rng.next() % 400) % LINES;
        let column = rng.next() % lines[line].len();
        lines[line].insert(column, 'x');
        highlighter.edit(line, 1, &[lines[line].as_str()]);
        let view_start = line.saturating_sub(VIEW / 2);
        let view_end = (view_start + VIEW).min(LINES);
        let start = Instant::now();
        black_box(highlighter.tokens(view_start, view_end));
        times.push(start.elapsed());
    }
    times.sort_by_key(|time| time.as_nanos());
    let p50 = times[times.len() / 2];
    let p99 = times[times.len() * 99 / 100];
    println!(
        "1,000 single-character edits + {VIEW}-line viewport: p50 {:.3} ms, p99 {:.3} ms",
        millis(p50),
        millis(p99)
    );
}
