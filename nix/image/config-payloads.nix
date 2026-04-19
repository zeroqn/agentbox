{ pkgs }:
{
  fishConfig = pkgs.writeTextDir "share/agentbox/fish/conf.d/agentbox-starship.fish" ''
    if status is-interactive
        starship init fish | source
    end
  '';

  starshipConfig = pkgs.writeTextDir "share/agentbox/starship.toml" ''
    [hostname]
    ssh_only = false
    format = "[$hostname]($style) "
    style = "bold green"
  '';
}
