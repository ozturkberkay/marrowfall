# Contributing

## Licensing your contribution

Marrowfall is GPL-3.0-or-later with the storefront SDK exception in
[LICENSE](LICENSE). It also ships on commercial storefronts, so every source
file linked into a build has to be covered by that exception. If any is not,
the shipped build cannot legally link the Steamworks SDK.

By contributing you confirm that:

1. You wrote the contribution, or you otherwise have the right to submit it
   under the project's license.
2. Your contribution is licensed under GPL-3.0-or-later **with the storefront
   SDK exception**, and the maintainer may distribute it under those terms,
   including in builds that link a proprietary storefront SDK.

Sign off every commit to certify this:

```sh
git commit -s
```

That appends a `Signed-off-by` line, which records your agreement to the
[Developer Certificate of Origin](https://developercertificate.org/) and to the
two points above.

Do not submit art assets. `art/` is proprietary; see [art/LICENSE](art/LICENSE).
