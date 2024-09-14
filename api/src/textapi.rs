pub fn split<const N: usize>(s: &str) -> [&str; N] {
    let mut res = [""; N];
    for (i, piece) in s.splitn(N, ' ').enumerate() {
        res[i] = piece
    }
    res
}
