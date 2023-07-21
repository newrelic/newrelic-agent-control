extern crate tera;
#[macro_use]
extern crate lazy_static;

use serde_json::{Map, Value};
use tera::{Context, Tera};
use std::error::Error;
use std::fs::File;
use std::io::Write;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Name of the person to greet
    #[arg(short, long)]
    dependencies: String,
    #[arg(short, long)]
    #[clap(default_value = "src/templates/*")]
    template_path: String,
    #[arg(short, long)]
    #[clap(default_value = "THIRD_PARTY_NOTICES.md")]
    output_file: String,
}

lazy_static! {
    pub static ref TEMPLATES: Tera = {
        let args = Args::parse();
        let tpl_path = args.template_path;
        let tera = match Tera::new(&tpl_path) {
            Ok(t) => t,
            Err(e) => {
                println!("Parsing error(s): {}", e);
                ::std::process::exit(1);
            }
        };
        tera
    };
}

fn main() -> Result<(), Box<dyn Error>>  {
    let args = Args::parse();
    let mut f = File::create(args.output_file)?;
    let markdown = render_markdown(args.dependencies);
    match markdown {
        Ok(s) => Ok(f.write_all(s.as_bytes())?),
        Err(e) => {
            println!("Error: {}", e);
            let mut cause = e.source();
            while let Some(e) = cause {
                println!("Reason: {}", e);
                cause = e.source();
            }
            Err("Error rendering the template".into())
        }
    }
}

fn render_markdown(data:String) -> Result<String, Box<dyn Error>> {
    let serialized = serde_json::from_str::<Map<String, Value>>(data.as_str()).unwrap();

    let mut context = Context::new();
    context.insert("dependencies", &serialized);

    // A one off template
    Tera::one_off("hello", &Context::new(), true).unwrap();

    Ok(TEMPLATES.render("THIRD_PARTY_NOTICES.md.tmpl", &context)?)
}
