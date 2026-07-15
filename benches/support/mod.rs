pub fn usize_to_u64(value: usize) -> u64 {
    u64::try_from(value).expect("benchmark size fits u64")
}
