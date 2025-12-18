{
  pkgs,
  ...
}:

{
  packages = with pkgs; [
    git
    k9s
    tilt
    just
    lldb
    docker
    colima
    kubectl
    kubectx
    kubernetes-helm
    vscode-extensions.vadimcn.vscode-lldb
  ];

  env = {
    CODELLDB_PATH = "${pkgs.vscode-extensions.vadimcn.vscode-lldb}/share/vscode/extensions/vadimcn.vscode-lldb/adapter/codelldb";
    LIBLLDB_PATH = "${pkgs.vscode-extensions.vadimcn.vscode-lldb}/share/vscode/extensions/vadimcn.vscode-lldb/lldb/lib/liblldb.dylib";
  };

  languages.rust = {
    enable = true;
    channel = "nightly";
  };

  scripts = {
    crdgen.exec = "cargo run --bin crdgen";
  };

  git-hooks.hooks = {
    rustfmt.enable = true;
    clippy.enable = true;
  };
}
