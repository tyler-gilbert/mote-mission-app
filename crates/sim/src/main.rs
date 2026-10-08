mod arguments;
mod runner;

fn main() -> anyhow::Result<()> {
    arguments::execute()
}
