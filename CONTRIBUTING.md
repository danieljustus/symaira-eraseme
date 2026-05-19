# Contributing to OpenEraseMe

## Code Contributions

1. Fork the repository.
2. Create a feature branch: `git checkout -b feature/my-feature`.
3. Install dependencies: `uv sync`.
4. Run tests: `pytest`.
5. Run lints: `ruff check . && mypy src/openeraseme/`.
6. Commit with a descriptive message.
7. Open a pull request.

### Code Style

- Python 3.11+ with type annotations.
- Format with `ruff format`.
- All CLI output must support `--output {text,json}`.
- All models must use pydantic v2.

## Broker Onboarding

Adding a new broker to the registry:

1. Create a YAML file in `registry/brokers/<jurisdiction>/`.
2. Validate against `registry/schemas/broker.schema.json`.
3. Include at least one verified opt-out channel (email or web form).
4. Provide verification keywords for auto-triage.
5. Open a PR.

See `registry/brokers/eu/_example.yaml` for the full reference.

## Testing

- Unit tests: `tests/unit/`
- Integration tests (require Docker/Mailpit): `tests/integration/`
- Registry validation: `tests/registry/`
- Fixtures in `tests/fixtures/`

Run all tests: `pytest`
