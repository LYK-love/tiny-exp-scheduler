fn main() {
    if let Err(err) = tiny_exp_scheduler::run_cli(std::env::args()) {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
