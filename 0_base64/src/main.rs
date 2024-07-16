mod base64;

use base64::Base64;
fn main() {
    let original = "abcde";
    let mut base64 = Base64::new();

    let encoded = base64.encode(original).unwrap();
    // let encoded = match base64.encode(original) {
    //     Ok(e) => e,
    //     Err(e) => panic!("{}", e),
    // };

    let decoded = base64.decode(&encoded).unwrap();
    // let decoded = match base64.decode(&encoded) {
    //     Ok(d) => d,
    //     Err(e) => panic!("{}", e),
    // };

    println!("Original: {}", original);
    println!("Encoded: {}", encoded);
    println!("Decoded: {}", decoded);
}

#[cfg(test)]
mod tests {
    use super::base64::Base64;

    #[test]
    fn test_base64_encode_decode() {
        let original = "hello world";
        let mut base64 = Base64::new();

        let encoded = base64.encode(original).unwrap();
        let decoded = base64.decode(&encoded).unwrap();

        assert_eq!(
            original, decoded,
            "Original and decoded messages do not match"
        );
    }
}
