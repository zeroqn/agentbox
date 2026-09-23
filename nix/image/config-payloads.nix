{ pkgs }:
{
  fishConfig = pkgs.writeTextDir "share/loftd/fish/conf.d/loftd-starship.fish" ''
    if status is-interactive
        starship init fish | source
    end
  '';

  starshipConfig = pkgs.writeTextDir "share/loftd/starship.toml" ''
    [hostname]
    ssh_only = false
    format = "[$hostname]($style) "
    style = "bold green"
  '';
}
