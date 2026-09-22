fn main() {
    if let Err(error) = teelens::run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
