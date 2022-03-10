# Bearclaw

An intercepting proxy for user-scripted, methodology-driven manual web application security testing.

## Status

In design and initial development. Not ready for use.

*File format and API will change between builds. There is not yet any backwards or forwards compatibility.*

## Motivation

I find existing security-focused intercepting proxies difficult to use for manual web application
testing, especially when trying to follow a methodology. Having to manually organize everything
in a spreadsheet is tedious, error prone, and takes up a lot of time that I'd rather spend on
testing. Furthermore, I get frustrated when I can't use a feature in an existing tool because my
target has some behavior the tool didn't anticipate.

What I really want is a tool that delegates the core functionality to a user script so I can
tailor the security testing to exactly fit the target application. Additionally, it should organize
everything according to a methodology of my choosing so I can keep track of what I've done and
what is still left to do. Finally, it should allow me to explore all the things in a nice UI.

## Goals

- Narrowly focused on scripted and manual testing
- Excellent UI
- Expect users to understand and write scripts as a normal part of their workflow
- Expose domain objects relevent to security testing to script; the user scripts them
- Organize testing and keep todo lists according to a user-defined methodology
- Collect, tie together, enrich, and expose all the things
- User can search, filter, aggregate, and drill down into all the things
- Conserve the user's precious RAM and disk space
- Don't freeze or crash
- Don't show incorrect or incomplete information
- Don't be unbearably slow

## Non-Goals

- Not a vulnerability scanner
- No automated issue detection
- Make no assumptions about web application behavior, delegate that to the user
- No extension support. We're user scripted, not third party scripted. Instead:
  - Consider creating a python library, it would be useful for our users as well as the
    greater community
  - If the application is missing a feature or doesn't expose a needed API or domain model to user
    scripting, please open an issue or make a contribution
- No centralized, curated, official, or endorsed repositories for sharing user scripts
- Don't try to be simple or minimal
- Don't hide anything from the user

## Dependencies

### Runtime and Compile-time

- GTK4
- GTKSourceView5
- OpenSSL
- Python
- Sqlite3
- Zstd

### Compile-time only

- Rust
- Capnproto (capnpc binary)

## License

Licensed under either of

 * Apache License, Version 2.0
   ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license
   ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Contributing

Contributions welcome!

Contributions should align with the project goals and avoid the non-goals. Please open an issue
to discuss large, impactful, or potentially controversial changes before starting work.

See [DESIGN.md](DESIGN.md) for a discussion on the overall design of the application.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the
work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.

## Copyright

Copyrights in the Bearclaw project are retained by their contributors. No copyright assignment is
required to contribute to the Bearclaw project.
