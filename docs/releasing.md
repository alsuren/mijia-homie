# Release procedure

To release a new version of `mijia-homie`:

1. Increment the version number in [Cargo.toml](mijia-homie/Cargo.toml), and push to master. You may also need to update the versions of the other crates in the repository.
2. Tag the commit which merges this to `master` to match the new version, like `mijia-homie-x.y.z`, and push to the repository.
3. Wait for the [Package workflow](https://github.com/alsuren/mijia-homie/actions?query=workflow%3APackage) to create a new draft [release](https://github.com/alsuren/mijia-homie/releases) including the Debian packages.
4. Edit the release, add an appropriate description, and then publish it.
5. As soon as the release is published, the packages should automatically be pushed to the [Artifactory repository](https://homiers.jfrog.io/) by the [Release workflow](https://github.com/alsuren/mijia-homie/actions?query=workflow%3ARelease).
