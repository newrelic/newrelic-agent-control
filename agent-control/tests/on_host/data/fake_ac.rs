fn main() {
    let args: Vec<String> = std::env::args().collect();
    eprintln!("started fake_ac with args: {:?}", args);
    if args.get(1).map(|s| s.as_str()) == Some("verify") {
        println!(r#"{{"message": "verification successful"}}"#);
    }
    eprintln!("finished fake_ac");
}
