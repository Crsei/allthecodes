use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));

    let result = if args.iter().any(|arg| arg == "--check") {
        allthecodes_protocol::codegen::check_frontend_api_artifacts_up_to_date(manifest_dir)
            .map(|()| allthecodes_protocol::codegen::default_frontend_artifact_paths(manifest_dir))
    } else {
        allthecodes_protocol::codegen::write_frontend_api_artifacts(manifest_dir)
    };

    match result {
        Ok(paths) => {
            for path in paths {
                println!("{}", path.display());
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}
