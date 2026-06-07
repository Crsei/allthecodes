use std::process::ExitCode;

fn main() -> ExitCode {
    match allthecodes_protocol::codegen::generate_openapi_json_pretty() {
        Ok(output) => {
            print!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}
