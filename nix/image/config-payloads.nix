{ pkgs }:
{
  fishConfig = pkgs.writeTextDir "share/cang/fish/conf.d/cang-starship.fish" ''
    if status is-interactive
        starship init fish | source
    end
  '';

  starshipConfig = pkgs.writeTextDir "share/cang/starship.toml" ''
    [hostname]
    ssh_only = false
    format = "[$hostname]($style) "
    style = "bold green"
  '';
}
