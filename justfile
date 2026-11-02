default:
    just --list

nix_flags := "--extra-experimental-features 'nix-command flakes'"

[macos]
dev-env flags="":
    @echo "Loading nadar dev env ... (extra flags: {{flags}})"
    nix {{nix_flags}} develop {{flags}} --command zsh

run analyser="":
    uv run jpamb analyse {{analyser}}

[macos]
flake-check:
    nix {{nix_flags}} flake check --all-systems

[macos]
nix-fmt:
    alejandra -q .

gc:
    nix-collect-garbage -d
    nix-collect-garbage --delete-older-than 7d
    nix-store --gc