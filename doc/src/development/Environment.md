# Environment

## VSCode

VSCode generally works well for developing bespin. The `rust-analyzer` plugin is
preferred over `rls` which often has build issues due to the project not having
a std runtime (`no-std`).

## Git

To have better usability when working with submodules, you can configure git to
update submodules automatically when doing a `git pull` etc.

```bash
git config --global submodule.recurse true
```

Fetch multiple submodules in parallel:

```bash
git config --global submodule.fetchJobs 20
```

We don't allow merge requests on master, to always keep a linear history. The
following alias can be helpful:

```gitconfig
[alias]
    purr = pull --rebase
```

### Adding a new submodule to the repository

1. `cd lib`
1. `git submodule add <path-to-repo> <foldername>`

### Removing a submodule in the repository

1. Delete the relevant section from the .gitmodules file.
1. Stage the .gitmodules changes: git add .gitmodules.
1. Delete the relevant section from .git/config.
1. Run git rm --cached path_to_submodule (no trailing slash).
1. Run rm -rf .git/modules/path_to_submodule (no trailing slash).
1. Commit changes