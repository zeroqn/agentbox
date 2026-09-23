{ self, pkgs }:
{
  default = {
    type = "app";
    program = "${self.packages.${pkgs.system}.cang}/bin/cang";
  };
}
