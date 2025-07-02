import subprocess
import re
import argparse
import json

# --- Test Configuration ---

# The different nesting depths to test, matching the Cargo features.
DEPTHS = [7, 9, 31, 63, 127, 255, 511, 513, 1023, 1025]

# The test configurations to run.
# (Test Name, Cargo Example Name, Extra Features)
CONFIGS = [
    ("serde", "test_serde", []),
    ("picojson-tiny", "test_picojson", ["pico-tiny"]),
    ("picojson-small", "test_picojson", ["pico-small"]),
    ("picojson-huge", "test_picojson", ["pico-huge"]),
]

def run_stack_analysis():
    """Runs the stack size analysis for different depths and configurations."""
    results = {}
    for depth in DEPTHS:
        depth_results = {}
        for name, example, extra_features in CONFIGS:
            print(f"Running {name} at depth {depth}...")

            # Construct the cargo command
            features = [f"depth-{depth}"] + extra_features
            feature_flags = " ".join([f"--features {f}" for f in features])
            command = f"cargo run --release --no-default-features {feature_flags} --example {example}"

            # Execute the command
            try:
                print(f"Running command: {command}")
                output = subprocess.check_output(command, shell=True, stderr=subprocess.STDOUT, universal_newlines=True)

                # Parse the output
                if "JSON parsing failed!" in output:
                    result_str = "Clean Fail"
                elif "=== TEST COMPLETE ===" in output:
                    match = re.search(r"Max stack usage: (\d+) bytes", output)
                    if match:
                        result_str = f"{match.group(1)} bytes"
                    else:
                        result_str = "Success (No Stack)"
                else:
                    result_str = "Stack Overflow"

            except subprocess.CalledProcessError as e:
                result_str = f"Build Failed: {e.output}"

            print(f"  Result: {result_str}")
            depth_results[name] = result_str

        results[depth] = depth_results
    return results

def print_stack_report(results):
    """Prints a markdown table of the stack analysis results."""
    header = "| Nesting Depth | " + " | ".join([c[0] for c in CONFIGS]) + "|"
    separator = "|---" * (len(CONFIGS) + 1) + "|"
    print("\n\n--- Stack Analysis Results ---")
    print(header)
    print(separator)

    for depth in sorted(results.keys()):
        row = f"| {depth} levels |"
        for name, _, _ in CONFIGS:
            row += f" {results[depth].get(name, 'N/A')} |"
        print(row)

def run_bloat_analysis():
    """Runs cargo-bloat and reports on binary size."""
    print("Running binary size analysis with cargo-bloat...")
    results = {}

    bloat_configs = [
        ("serde", "test_serde", []),
        ("picojson", "test_picojson", []),
    ]

    # Bloat analysis doesn't depend on nesting depth, so we run it once for each config.
    for name, example, extra_features in bloat_configs:
        print(f"Running bloat for {name}...")

        # Construct the cargo bloat command
        features = extra_features
        feature_flags = " ".join([f"--features {f}" for f in features])
        command = f"cargo bloat --release {feature_flags} --example {example} --message-format=json"

        try:
            print(f"Running command: {command}")
            output = subprocess.check_output(command, shell=True, stderr=subprocess.STDOUT, universal_newlines=True)
            # Get the last line of output, which should be the JSON
            json_output = output.strip().split('\n')[-1]
            data = json.loads(json_output)
            # The file size is in the 'text-section-size' field of the JSON output
            file_size = data.get('text-section-size', 0)
            file_size_kb = file_size / 1024
            results[name] = f"{file_size_kb:.1f} KB"

        except subprocess.CalledProcessError as e:
            results[name] = f"Bloat Failed: {e.output}"
        except (json.JSONDecodeError, IndexError):
            results[name] = "Bloat Failed: Invalid JSON"

    return results

def print_bloat_report(results):
    """Prints a markdown table of the bloat analysis results."""
    header = "| Configuration | Binary Size |"
    separator = "|---|---|"
    print("\n\n--- Binary Size Analysis (cargo-bloat) ---")
    print(header)
    print(separator)

    for name, size in results.items():
        row = f"| {name} | {size} |"
        print(row)

def main():
    """Main entry point for the script."""
    parser = argparse.ArgumentParser(description="Run test suites for the picojson-rs crate.")
    parser.add_argument(
        "tool",
        nargs='?',
        default="stack",
        choices=["stack", "bloat"],
        help="The analysis tool to run: 'stack' for stack size analysis, 'bloat' for binary size analysis."
    )
    args = parser.parse_args()

    if args.tool == "stack":
        results = run_stack_analysis()
        print_stack_report(results)
    elif args.tool == "bloat":
        results = run_bloat_analysis()
        print_bloat_report(results)

if __name__ == "__main__":
    main()
