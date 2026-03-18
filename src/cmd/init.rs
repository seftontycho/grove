use anyhow::Result;

use crate::config::Shell;

pub fn run(shell: &Shell) -> Result<()> {
    let output = match shell {
        Shell::Zsh | Shell::Bash => zsh_bash_function(),
        Shell::Fish => fish_function(),
    };
    print!("{output}");
    Ok(())
}

fn zsh_bash_function() -> &'static str {
    r#"# Grove shell integration
# Add to your .zshrc or .bashrc:
#   eval "$(grove init zsh)"

gv() {
  if [ $# -eq 0 ]; then
    grove open
  else
    case "$1" in
      -l) grove session list ;;
      -c) shift; grove tree close "$@" ;;
      *)  grove open "$@" ;;
    esac
  fi
}
"#
}

fn fish_function() -> &'static str {
    r#"# Grove shell integration
# Add to your config.fish:
#   grove init fish | source

function gv
    if test (count $argv) -eq 0
        grove open
    else
        switch $argv[1]
            case -l
                grove session list
            case -c
                grove tree close $argv[2..]
            case '*'
                grove open $argv
        end
    end
end
"#
}
