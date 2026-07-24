import subprocess
import argparse
import json

def run_bloat_analysis():
    """Runs cargo-bloat and reports on binary size."""
    print("Running binary size analysis with cargo-bloat...")
    results = {}

    bloat_configs = [
        ("serde", "test_serde", ["int8"]),
        ("picojson-slice", "test_picojson", ["pico-tiny", "int8"]),
        ("picojson-stream", "test_streamparser", ["pico-tiny", "int8"]),
    ]

    # Bloat analysis doesn't depend on nesting depth, so we run it once for each config.
    for name, example, extra_features in bloat_configs:
        print(f"Running bloat for {name}...")

        # Construct the cargo bloat command
        command = ["cargo", "bloat", "--release", "--message-format=json"]
        if extra_features:
            command.extend(
                (
                    "--no-default-features",
                    "--features",
                    ",".join(extra_features),
                )
            )
        command.extend(["--example", example])

        try:
            print(f"Running command: {' '.join(command)}")
            output = subprocess.check_output(command, stderr=subprocess.STDOUT, universal_newlines=True)
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
        default="bloat",
        choices=["bloat"],
        help="Run the remaining binary-size analysis. Stack and panic audits use cargo krabi-caliper."
    )
    args = parser.parse_args()

    results = run_bloat_analysis()
    print_bloat_report(results)

if __name__ == "__main__":
    main()
