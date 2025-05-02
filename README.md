# Pubscan

Pubscan is a tool to analyze internal APIs of Python packages. It's built on top of [ruff](https://github.com/astral-sh/ruff).

## Example

```
uvx pubscan <path-to-package>
```

## Motivation

Python does not have a public keyword. The public surface area of a package is whatever is imported by other packages.

`pubscan` shows the effective public surface area of a package. As an onboarding engineer, this is useful to figure out what parts of a package are the most important. As a package maintainer, this is useful to figure out what parts of a package are used elsewhere.

## License<a id="license"></a>

This repository is licensed under the [MIT License](https://github.com/vivster7/pubscan/blob/main/LICENSE)
