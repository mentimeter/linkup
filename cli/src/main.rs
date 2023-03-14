use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(author, version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Start {
        #[arg(short, long)]
        config: Option<String>,
    },
    Stop {

    },
    Check {

    },
    Local {

    },
    Remote {

    },
}

fn main() {
    let cli = Cli::parse();

    match &cli.command {
       Commands::Start{config}=> {
        match config {
            Some(c) => {
                println!("had some, {}", c)
            }
            None => {
                println!("had none config")
            }
        }
        println!("Start with config {:?}", config)
       },
       Commands::Stop{} => println!("Stop"),
       Commands::Check{} => println!("Check"),
       Commands::Local{} => println!("Local"),
       Commands::Remote{} => println!("Remote")

    //    _Stop => println!("Stop"),
    //    _Check => println!("Check"),
    //    _Local => println!("Local"),
    }

}
