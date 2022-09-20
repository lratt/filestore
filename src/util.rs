pub fn gen_key() -> String {
    use nanorand::Rng;
    let mut rng = nanorand::tls_rng();
    let nums = std::iter::repeat_with(|| rng.generate::<u8>())
        .take(5)
        .collect::<Vec<_>>();
    base64::encode_config(
        &nums,
        base64::Config::new(base64::CharacterSet::UrlSafe, false),
    )
}
