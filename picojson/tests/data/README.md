# Test Data Sources

This directory contains downloaded JSON conformance test suites used for comprehensive testing of the picojson parser.

## Test Suites

### JSONTestSuite
- **Source**: https://github.com/nst/JSONTestSuite
- **Location**: `tests/data/JSONTestSuite/`
- **Description**: A comprehensive test suite for JSON parsers containing hundreds of test cases
- **Test Categories**:
  - `y_*.json` - Tests that should parse successfully
  - `n_*.json` - Tests that should fail to parse
  - `i_*.json` - Implementation-dependent tests (behavior may vary)
- **Feature Flag**: `remote-tests`

### JSON_checker
- **Source**: http://www.json.org/JSON_checker/
- **Location**: `tests/data/json_checker/`
- **Description**: The original JSON validation test suite from json.org
- **Test Categories**:
  - `pass*.json` - Tests that should parse successfully
  - `fail*.json` - Tests that should fail to parse
- **Feature Flag**: `json-checker-tests`

## Usage

To run tests with both suites:
```bash
cargo test --features="remote-tests,json-checker-tests"
```

To run only JSONTestSuite tests:
```bash
cargo test --features="remote-tests"
```

To run only JSON_checker tests:
```bash
cargo test --features="json-checker-tests"
```

## Automatic Download

Test suites are automatically downloaded during build when the corresponding feature flags are enabled. Downloaded files are cached and won't be re-downloaded unless the directories are removed.

## Generated Tests

The build script (`build.rs`) automatically generates Rust test functions from the JSON files in these directories. Generated tests are written to `tests/conformance_generated.rs`.
