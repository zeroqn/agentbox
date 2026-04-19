{ self, pkgs }:
{
  default = {
    type = "app";
    program = "${self.packages.${pkgs.system}.agentbox}/bin/agentbox";
  };
}
