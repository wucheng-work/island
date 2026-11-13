//! Decimal file sizes for the island's file and system views.

/// "4.1 MB", "820 KB", "12 bytes": sizes as the island shows them.
pub fn fmt_bytes(n: u64) -> String {
    const K: f64 = 1000.0;
    let f = n as f64;
    if n < 1000 {
        format!("{n} {}", if n == 1 { "byte" } else { "bytes" })
    } else if f < K * K {
        format!("{:.0} KB", f / K)
    } else if f < K * K * K {
        let mb = f / (K * K);
        if mb < 10.0 {
            format!("{mb:.1} MB")
        } else {
            format!("{mb:.0} MB")
        }
    } else {
        format!("{:.1} GB", f / (K * K * K))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_sizes() {
        assert_eq!(fmt_bytes(0), "0 bytes");
        assert_eq!(fmt_bytes(1), "1 byte");
        assert_eq!(fmt_bytes(999), "999 bytes");
        assert_eq!(fmt_bytes(820_400), "820 KB");
        assert_eq!(fmt_bytes(4_120_000), "4.1 MB");
        assert_eq!(fmt_bytes(84_000_000), "84 MB");
        assert_eq!(fmt_bytes(1_260_000_000), "1.3 GB");
    }
}
