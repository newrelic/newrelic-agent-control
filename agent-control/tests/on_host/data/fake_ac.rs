fn main() {
    let args: Vec<String> = std::env::args().collect();
    eprintln!("started fake_ac with args: {:?}", args);
    match args.get(1).map(|s| s.as_str()) {
        Some("verify") => println!(r#"{{"message": "verification successful"}}"#),
        Some("id") => println!("{}", env!("FAKE_AC_TEST_ID")),
        _ => println!("unknown command"),
    };
    eprintln!("finished fake_ac");
}
