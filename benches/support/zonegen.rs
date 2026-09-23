//! Generates large, deterministic zone files for benchmarks and profiling.
//!
//! Shared by `benches/parse.rs` and `examples/genzone.rs` through `#[path]`.

use std::fmt::Write as _;

/// Builds a deterministic zone with roughly `records` records under
/// `example.com.`, mixing in the syntax a real zone uses: comments,
/// parentheses, quoted strings, inherited owners, TTL units and escapes.
pub fn generate(records: usize) -> String {
    let mut out = String::with_capacity(records * 48);
    out.push_str(
        "$ORIGIN example.com.\n\
         $TTL 86400\n\
         @\tSOA\tns1.example.com.\thostmaster.example.com. (\n\
         \t\t2026092301 ; serial\n\
         \t\t21600      ; refresh\n\
         \t\t3600       ; retry\n\
         \t\t604800     ; expire\n\
         \t\t86400 )    ; minimum\n\
         \tNS\tns1.example.com.\n\
         \tNS\tns2.example.com.\n",
    );

    let mut count = 3;
    let mut i = 0usize;
    while count < records {
        let ip = format!("10.{}.{}.{}", (i >> 16) & 255, (i >> 8) & 255, i & 255);
        let _ = match i % 8 {
            0 => {
                count += 1;
                writeln!(out, "host{i}\tA\t{ip}\n\tAAAA\t2001:db8::{i:x}")
            }
            1 => writeln!(out, "host{i}\tCNAME\thost{}", i - 1),
            2 => writeln!(out, "host{i}\t3600\tIN\tMX\t10 mail{i}.example.com."),
            3 => writeln!(
                out,
                "host{i}\tTXT\t\"v=spf1 include:_spf.example.com ~all; x\""
            ),
            4 => writeln!(out, "; host{i} moves next week\nhost{i}\t1h30m\tA\t{ip}"),
            5 => writeln!(out, "host{i}\\.a\tIN\tA\t{ip}"),
            6 => writeln!(out, "host{i}\tTXT\t( \"part one\"\n\t\t\"part two\" )"),
            _ => writeln!(out, "host{i}\tA\t{ip} ; trailing comment"),
        };
        count += 1;
        i += 1;
    }
    out
}
