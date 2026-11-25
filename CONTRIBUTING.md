# Contributing

## Licence and sign-off

The project is licensed under [Apache-2.0](LICENSE). By contributing you agree that your contribution is licensed under it too.

Sign off every commit to certify the [Developer Certificate of Origin](https://developercertificate.org/), version 1.1:

```sh
git commit -s -m "fix(cli): describe the change"
```

This adds a `Signed-off-by: Your Name <you@example.org>` line, using the name and email from your git configuration. Pull requests with unsigned commits are asked to add the sign-off before merging.

## Commits and pull requests

- Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/): a type such as `feat`, `fix`, `docs`, `ci` or `build`, an optional scope, then a short description. release-please builds the changelog and the next version from them.
- One pull request per change set, with atomic commits. CI runs `make lint` and `make test`.
- New dependencies must carry a licence that [deny.toml](deny.toml) allows; the `licenses` workflow checks this with cargo-deny.

## Building

See [docs/details.md](docs/details.md#build): everything runs in containers (`make build`, `make lint`, `make test`).
