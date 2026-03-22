# Documentation

Complete documentation for the Asset Tap project.

## Quick Links

- **[Main README](../README.md)** - Installation, quick start, and basic usage
- **[Development Guide](DEVELOPMENT.md)** - Developer setup and contribution workflow

## User Guides

Documentation for users of the application:

### [Bundle Structure](guides/BUNDLE_STRUCTURE.md)

Understanding the output file format and metadata structure. Learn how generated assets are organized and what each file contains.

**Topics:**

- Directory structure
- Metadata format (bundle.json)
- File naming conventions
- Loading bundles in code

## Technical Documentation

Documentation for developers and advanced users:

### [Provider System](architecture/PROVIDERS.md)

Deep-dive into the YAML-based provider plugin architecture. Learn how providers work and how to add your own.

**Topics:**

- Architecture overview
- Provider configuration
- Response types (Json, Binary, Base64, Polling)
- Upload system (multipart, initiate-then-put)
- Extensible provider system with pre-configured integrations
- Adding custom providers

### [Provider Schema Reference](guides/PROVIDER_SCHEMA.md)

Complete YAML schema reference for creating and configuring provider plugins.

**Topics:**

- Schema structure
- Provider metadata
- Model configuration
- Request templates
- Response templates
- Upload configuration
- Best practices
- Validation and testing

## Development

### [Development Guide](DEVELOPMENT.md)

Complete guide for local development and contributing.

**Topics:**

- Development setup
- Project structure
- Development workflow
- Testing practices
- Code standards
- Adding features (providers, templates, GUI)
- Submitting changes

### [CLAUDE.md](../CLAUDE.md)

Optimized guide for AI-assisted development with Claude Code.

**Topics:**

- Project overview
- Essential commands
- Architecture essentials
- Development practices
- Common gotchas
- Key principles

## Additional Resources

### Code Documentation

Generate and browse the full Rust API documentation:

```bash
make doc        # Generate docs
make doc-open   # Generate and open in browser
```

### Example Configurations

- **Providers**: See `providers/*.yaml` for pre-configured provider integrations
- **Templates**: See `templates/humanoid.yaml` and other files in `templates/`

### Source Code

- **Core Library**: `core/src/`
  - Provider system: `core/src/providers/`
  - Template system: `core/src/templates/`
  - Pipeline orchestration: `core/src/pipeline.rs`
  - Blender integration: `core/src/convert.rs`
- **CLI**: `cli/src/main.rs`
- **GUI**: `gui/src/`

## Documentation Maintenance

### When to Update Docs

- **README.md**: User-facing features, installation changes, quick start updates
- **CLAUDE.md**: Architecture changes, development workflow changes, new gotchas
- **DEVELOPMENT.md**: Developer setup changes, new tools, testing practices
- **Provider docs**: New providers, schema changes, new features
- **Bundle Structure**: Metadata format changes, new file types

### Style Guidelines

- **User guides**: Focus on "how to" with examples
- **Technical docs**: Explain "why" and "how it works"
- **Code examples**: Always test before documenting
- **Links**: Use relative links within docs
- **Formatting**: Follow dprint rules (2 spaces for Markdown)

## Need Help?

- **Questions**: [GitHub Discussions](https://github.com/nightandwknd/asset-tap/discussions)
- **Bugs**: [GitHub Issues](https://github.com/nightandwknd/asset-tap/issues)
- **Contributing**: See [DEVELOPMENT.md](DEVELOPMENT.md)
