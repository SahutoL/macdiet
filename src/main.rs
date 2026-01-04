fn main() {
    if let Err(err) = macdiet::cli::run() {
        macdiet::ui::eprintln_error(&err);
        std::process::exit(macdiet::exit::exit_code(&err));
    }
}
