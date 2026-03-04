use anyhow::{Result, anyhow};
use clap::{Args, Command};
use clap_mangen::Man;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Args, Debug)]
pub(crate) struct ManArgs {
    /// Output directory to write generated man pages
    #[arg(short = 'o', long)]
    pub(crate) output: Option<PathBuf>,

    /// Man section number to use in generated filenames
    #[arg(long, default_value_t = 1)]
    pub(crate) section: u8,
}

/// Run the `man` command.
///
/// # Errors
///
/// Returns an error if man-page generation fails or output files cannot be written.
pub(crate) fn run(root_command: Command, args: &ManArgs) -> Result<()> {
    let pages = build_man_pages(root_command, args.section)?;
    if let Some(output_dir) = args.output.as_deref() {
        write_pages(output_dir, &pages)?;
        return Ok(());
    }

    let root_page = pages
        .iter()
        .find(|page| page.file_stem == "99problems")
        .ok_or_else(|| anyhow!("generated man pages did not contain a root page"))?;
    print!("{}", root_page.contents);
    Ok(())
}

#[derive(Debug, Clone)]
struct ManPage {
    file_stem: String,
    section: u8,
    contents: String,
}

fn build_man_pages(mut root: Command, section: u8) -> Result<Vec<ManPage>> {
    root.build();
    let root_name = root.get_name().to_string();

    let mut pages = vec![render_page(root.clone(), root_name.clone(), section)?];
    for sub in root.get_subcommands().cloned() {
        let file_stem = format!("{root_name}-{}", sub.get_name());
        pages.push(render_page(sub, file_stem, section)?);
    }
    pages.sort_by(|a, b| a.file_stem.cmp(&b.file_stem));
    Ok(pages)
}

fn render_page(command: Command, file_stem: String, section: u8) -> Result<ManPage> {
    let mut buffer = Vec::new();
    let man = Man::new(command);
    man.render(&mut buffer)?;
    let contents =
        String::from_utf8(buffer).map_err(|_| anyhow!("generated man page was not valid UTF-8"))?;
    Ok(ManPage {
        file_stem,
        section,
        contents,
    })
}

fn write_pages(output_dir: &Path, pages: &[ManPage]) -> Result<()> {
    fs::create_dir_all(output_dir)?;
    for page in pages {
        let file_name = format!("{}.{}", page.file_stem, page.section);
        let path = output_dir.join(file_name);
        let mut file = fs::File::create(path)?;
        file.write_all(page.contents.as_bytes())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[derive(clap::Parser, Debug)]
    struct ExampleCli {
        #[command(subcommand)]
        command: ExampleCommands,
    }

    #[derive(clap::Subcommand, Debug)]
    enum ExampleCommands {
        Get,
        Man(ManArgs),
    }

    #[test]
    fn build_pages_includes_root_and_subcommands() {
        let command = ExampleCli::command();
        let root_name = command.get_name().to_string();
        let pages = build_man_pages(command, 1).expect("expected man generation");
        assert!(pages.iter().any(|p| p.file_stem == root_name));
        assert!(
            pages
                .iter()
                .any(|p| p.file_stem == format!("{root_name}-get"))
        );
        assert!(
            pages
                .iter()
                .any(|p| p.file_stem == format!("{root_name}-man"))
        );
    }
}
