import subprocess
import re
import argparse
import json
import sys
import os

def get_depths_from_build_rs():
    """Parses build.rs to extract the DEPTHS constant."""
    try:
        with open("build.rs", "r") as f:
            content = f.read()
            match = re.search(r"const DEPTHS: &\[usize\] = &\[(.*?)\];", content, re.DOTALL)
            if match:
                depths_str = match.group(1).replace('\n', '').replace(',', ' ').split()
                return [int(d) for d in depths_str]
            else:
                # No match found - return empty list for consistency
                return []
    except (IOError, ValueError) as e:
        print(f"Could not read or parse DEPTHS from build.rs: {e}", file=sys.stderr)
        return []  # Return a default or empty list

# --- Test Configuration ---

# The different nesting depths to test, matching the Cargo features.
DEPTHS = get_depths_from_build_rs()

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
            command = ["cargo", "run", "--release", "--no-default-features"]
            if features:
                command.append("--features")
                command.append(",".join(features))
            command.extend(["--example", example])

            # Execute the command
            try:
                print(f"Running command: {' '.join(command)}")
                output = subprocess.check_output(command, stderr=subprocess.STDOUT, universal_newlines=True)

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

            except UnicodeDecodeError:
                # Handle case where output contains binary garbage (stack overflow)
                result_str = "Stack Overflow (Binary Output)"
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
        command = ["cargo", "bloat", "--release", "--message-format=json"]
        if extra_features:
            command.append("--features")
            command.append(",".join(extra_features))
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

def run_panic_checker(example_name, profile="panic_checks", verbose=False, no_default_features=False, features=None):
    """Run panic checker on a specific example."""
    # Panic-related patterns to search for (specific function names only)
    panic_patterns = [
        r'panic_fmt',
        r'panic_const',
        r'panic_nounwind',
        r'panic_impl',
        r'assert_failed',
        r'unwrap_failed',
        r'expect_failed',
        r'slice_end_index_len_fail',
        r'slice_start_index_len_fail',
        r'slice_index_len_fail',
        r'panic_for_nonpositive_argument',
        r'panic_bounds_check',
        r'unreachable_unchecked',
        r'core::panicking::',
        r'panic!',
        r'unwrap\(\)',
        r'expect\(',
    ]

    print(f"🔍 Checking example '{example_name}' for panic references...")

    try:
        # Build the objdump command
        cmd = [
            "cargo", "objdump",
            "--profile", profile
        ]

        # Add feature flags if specified
        if no_default_features:
            cmd.append("--no-default-features")
        if features:
            cmd.extend(["--features", features])

        cmd.extend([
            "--example", example_name,
            "--", "-dS"
        ])

        if verbose:
            print(f"Running: {' '.join(cmd)}")

        result = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=120  # 2 minute timeout
        )

        if result.returncode != 0:
            print(f"❌ Error running objdump: {result.stderr}", file=sys.stderr)
            return False

        # Filter objdump output to only include content after .elf file format line
        lines = result.stdout.split('\n')
        start_idx = None

        for i, line in enumerate(lines):
            if '.elf:' in line and 'file format' in line:
                start_idx = i
                break

        if start_idx is None:
            print("Warning: Could not find .elf file format marker", file=sys.stderr)
            filtered_output = result.stdout
        else:
            filtered_output = '\n'.join(lines[start_idx:])

        # Save filtered assembly output to file
        output_dir = f"target/avr-none/{profile}/examples"
        os.makedirs(output_dir, exist_ok=True)
        asm_file = f"{output_dir}/{example_name}.asm"

        try:
            with open(asm_file, 'w') as f:
                f.write(filtered_output)
            print(f"💾 Assembly saved to: {asm_file}")
        except Exception as e:
            print(f"⚠️  Warning: Could not save assembly file: {e}")

        # Check for panic patterns and track function context
        found_panics = []
        lines = filtered_output.split('\n')
        current_function = None

        for line_num, line in enumerate(lines, 1):
            # Track function/symbol boundaries (lines that contain < and > with function names)
            function_header_line = False
            if '<' in line and '>' in line and line.endswith(':'):
                # Extract function name from lines like "0000034c <core::option::unwrap_failed::h71083ebf777fe1d3>:"
                # Handle nested angle brackets like "<<avr_hal_generic::delay::Delay<SPEED>...>>"
                match = re.search(r'<(.+)>:', line)
                if match:
                    current_function = match.group(1)
                    function_header_line = True

            # Check for panic patterns
            for pattern in panic_patterns:
                if re.search(pattern, line, re.IGNORECASE):
                    # Build context info - but NOT for function header lines themselves
                    if function_header_line:
                        context_info = ""  # Don't add context to function headers
                    else:
                        context_info = f" [from {current_function}]" if current_function else ""

                    found_panics.append(f"Line {line_num}: {line.strip()}{context_info}")
                    if verbose:
                        print(f"Found panic pattern '{pattern}' at line {line_num}: {line.strip()}{context_info}")
                    break  # Only report each line once

        if found_panics:
            print(f"❌ FAIL: Found {len(found_panics)} panic reference(s) in '{example_name}':")
            for ref in found_panics:
                # Extract line number from the format "Line X: content"
                line_match = ref.split(": ", 1)
                if len(line_match) == 2:
                    line_part = line_match[0]  # "Line X"
                    content = line_match[1]    # actual content
                    line_num = line_part.replace("Line ", "")
                    # Output in IDE-friendly format: filename:line_number: message
                    print(f"{asm_file}:{line_num}: {content}")
                else:
                    print(f"  {ref}")
            return False
        else:
            print(f"✅ PASS: No panic references found in '{example_name}'")
            return True

    except subprocess.TimeoutExpired:
        print("❌ Error: objdump command timed out", file=sys.stderr)
        return False
    except Exception as e:
        print(f"❌ Error running objdump: {e}", file=sys.stderr)
        return False

def get_available_examples():
    """Auto-discover available examples from the examples/ directory."""
    examples = []
    examples_dir = "examples"

    if os.path.exists(examples_dir):
        for file in os.listdir(examples_dir):
            if file.endswith('.rs'):
                example_name = file[:-3]  # Remove .rs extension
                examples.append(example_name)

    return sorted(examples)

def run_panic_analysis(specific_examples=None):
    """Run panic checker on specified examples or all available ones."""
    if specific_examples:
        examples = specific_examples
    else:
        examples = get_available_examples()

    results = {}

    print(f"\n=== Panic Reference Analysis ===")
    print(f"Checking {len(examples)} example(s): {', '.join(examples)}")

    for example in examples:
        # Check if example exists
        example_path = f"examples/{example}.rs"
        if not os.path.exists(example_path):
            print(f"⚠️  Skipping {example} - file not found")
            results[example] = "Not Found"
            continue

        success = run_panic_checker(example, verbose=False)
        results[example] = "✅ PASS" if success else "❌ FAIL"
        print()  # Add spacing between examples

    return results

def print_panic_report(results):
    """Print a summary of panic check results."""
    header = "| Example | Panic Check Result |"
    separator = "|---|---|"
    print("\n--- Panic Analysis Summary ---")
    print(header)
    print(separator)

    for example, result in results.items():
        print(f"| {example} | {result} |")

    # Overall status
    failed_count = sum(1 for r in results.values() if "FAIL" in r)
    total_count = len([r for r in results.values() if r != "Not Found"])

    if failed_count > 0:
        print(f"\n❌ OVERALL: {failed_count}/{total_count} examples have panic references")
        return False
    else:
        print(f"\n✅ OVERALL: All {total_count} examples are panic-free!")
        return True

def main():
    """Main entry point for the script."""
    parser = argparse.ArgumentParser(description="Run test suites for the picojson-rs crate.")
    parser.add_argument(
        "tool",
        nargs='?',
        default="stack",
        choices=["stack", "bloat", "panic"],
        help="The analysis tool to run: 'stack' for stack size analysis, 'bloat' for binary size analysis, 'panic' for panic reference checking."
    )
    parser.add_argument(
        "--quick",
        action="store_true",
        help="Quick mode: only test the first depth (7) for faster iteration"
    )
    parser.add_argument(
        "--example",
        help="For panic checking: specify a single example to check (e.g., 'minimal')"
    )
    parser.add_argument(
        "--examples",
        action="store_true",
        help="For panic checking: check all available examples"
    )
    parser.add_argument(
        "--verbose", "-v",
        action="store_true",
        help="Verbose output for panic checking"
    )
    parser.add_argument(
        "--no-default-features",
        action="store_true",
        help="For panic checking: disable default features (equivalent to cargo --no-default-features)"
    )
    parser.add_argument(
        "--features",
        help="For panic checking: comma-separated list of features to enable (equivalent to cargo --features)"
    )
    args = parser.parse_args()

    if args.tool == "stack":
        # Use only first depth if quick mode is enabled
        global DEPTHS
        if args.quick:
            original_depths = DEPTHS
            DEPTHS = [DEPTHS[0]] if DEPTHS else [7]  # Use first depth or fallback to 7
            print(f"Quick mode: Testing only depth {DEPTHS[0]}")

        results = run_stack_analysis()
        print_stack_report(results)

        # Restore original depths
        if args.quick:
            DEPTHS = original_depths

    elif args.tool == "bloat":
        results = run_bloat_analysis()
        print_bloat_report(results)

    elif args.tool == "panic":
        if args.example:
            # Check single example
            success = run_panic_checker(
                args.example,
                verbose=args.verbose,
                no_default_features=args.no_default_features,
                features=args.features
            )
            sys.exit(0 if success else 1)
        elif args.examples:
            # Check all available examples
            results = run_panic_analysis()
            success = print_panic_report(results)
            sys.exit(0 if success else 1)
        else:
            # Show available examples and usage
            available = get_available_examples()
            print("Available examples for panic checking:")
            for example in available:
                print(f"  - {example}")
            print(f"\nUsage:")
            print(f"  python run_suite.py panic --example <name>                                    # Check specific example")
            print(f"  python run_suite.py panic --example <name> --no-default-features             # Check without default features")
            print(f"  python run_suite.py panic --example <name> --features depth-7,pico-tiny      # Check with specific features")
            print(f"  python run_suite.py panic --examples                                          # Check all examples")
            sys.exit(0)

if __name__ == "__main__":
    main()
