{ self, pkgs }:
{
  default = {
    type = "app";
    program = "${self.packages.${pkgs.system}.loftd}/bin/loftd";
  };
}
