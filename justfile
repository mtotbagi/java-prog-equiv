default:
    just --list

nix_flags := "--extra-experimental-features 'nix-command flakes'"

[macos]
dev-env flags="":
    @echo "Loading nadar dev env ... (extra flags: {{flags}})"
    nix {{nix_flags}} develop {{flags}} --command zsh

run analyser="":
    uv run jpamb analyse {{analyser}}

# Run the eqbench CLI, e.g. `just eqbench details caldat-caldat-Neq`
eqbench *args:
    cargo run -q -p eqbench -- {{args}}

# Plot eqbench results into eqbench/results/plots, e.g. `just eqbench-plot eqbench/results/*-300.jsonl`
eqbench-plot *files="eqbench/results/*.jsonl":
    nix {{nix_flags}} run ./eqbench/plots -- --out eqbench/results/plots {{files}}

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