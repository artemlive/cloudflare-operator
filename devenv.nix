{
  pkgs,
  ...
}:

{
  packages = [ pkgs.git ];

  languages.rust = {
    enable = true;
    channel = "nightly";
  };

  git-hooks.hooks = {
    rustfmt.enable = true;
    clippy.enable = true;
  };
}
