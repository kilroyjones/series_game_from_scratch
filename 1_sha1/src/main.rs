mod sha1;

use sha1::Sha1;

fn main() {
    let mut hasher = Sha1::new();
    let res = hasher.hash("knownhash".to_owned());
    println!("{:?}", res);
}
