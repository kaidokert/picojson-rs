import subprocess
import re
import argparse
import json
import sys
import os

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

def _run_objdump(example_name, profile, verbose, no_default_features, features):
    """Executes the cargo objdump command and returns the process result."""
    cmd = ["cargo", "objdump", "--profile", profile]
    if no_default_features:
        cmd.append("--no-default-features")
    if features:
        cmd.extend(["--features", features])
    cmd.extend(["--example", example_name, "--", "-dS","-l","-z","--show-all-symbols"])

    if verbose:
        print(f"Running: {' '.join(cmd)}")

    return subprocess.run(
        cmd,
        capture_output=True,
        text=True,
        timeout=120  # 2 minute timeout
    )

def _filter_objdump_output(stdout):
    """Extracts relevant sections from the objdump output."""
    lines = stdout.split('\n')
    start_idx = None
    for i, line in enumerate(lines):
        if '.elf:' in line and 'file format' in line:
            start_idx = i
            break
    if start_idx is None:
        print("Warning: Could not find .elf file format marker", file=sys.stderr)
        return stdout
    return '\n'.join(lines[start_idx:])

def _save_assembly_output(example_name, profile, content):
    """Writes the filtered output to a file and returns the file path."""
    output_dir = f"target/avr-none/{profile}/examples"
    os.makedirs(output_dir, exist_ok=True)
    asm_file = f"{output_dir}/{example_name}.asm"
    try:
        with open(asm_file, 'w') as f:
            f.write(content)
        print(f"💾 Assembly saved to: {asm_file}")
    except Exception as e:
        print(f"⚠️  Warning: Could not save assembly file: {e}")
    return asm_file

def _analyze_panic_patterns(content, verbose):
    """Scans the output for panic patterns and returns found references."""
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
    found_panics = []
    lines = content.split('\n')
    current_function = None
    for line_num, line in enumerate(lines, 1):
        # Skip disassembler comments that contain false positive int_log10 references
        if re.match(r'^ *;.*int_log10::panic_for_nonpositive_argument', line):
            if verbose:
                print(f"Skipping false positive at line {line_num}: {line.strip()}")
            continue

        function_header_line = False
        if '<' in line and '>' in line and line.endswith(':'):
            match = re.search(r'<(.+)>:', line)
            if match:
                current_function = match.group(1)
                function_header_line = True
        for pattern in panic_patterns:
            if re.search(pattern, line, re.IGNORECASE):
                context_info = "" if function_header_line else f" [from {current_function}]" if current_function else ""
                found_panics.append(f"Line {line_num}: {line.strip()}{context_info}")
                if verbose:
                    print(f"Found panic pattern '{pattern}' at line {line_num}: {line.strip()}{context_info}")
                break
    return found_panics

def _report_panic_results(example_name, asm_file, found_panics):
    """Prints the results and returns the final boolean."""
    if found_panics:
        print(f"❌ FAIL: Found {len(found_panics)} panic reference(s) in '{example_name}':")
        for ref in found_panics:
            line_match = ref.split(": ", 1)
            if len(line_match) == 2:
                line_part, content = line_match
                line_num = line_part.replace("Line ", "")
                print(f"{asm_file}:{line_num}: {content}")
            else:
                print(f"  {ref}")
        return False
    else:
        print(f"✅ PASS: No panic references found in '{example_name}'")
        return True

def run_panic_checker(example_name, profile="dev", verbose=False, no_default_features=False, features=None):
    """Run panic checker on a specific example."""
    print(f"🔍 Checking example '{example_name}' for panic references...")
    try:
        result = _run_objdump(example_name, profile, verbose, no_default_features, features)
        if result.returncode != 0:
            print(f"❌ Error running objdump: {result.stderr}", file=sys.stderr)
            return False

        filtered_output = _filter_objdump_output(result.stdout)
        asm_file = _save_assembly_output(example_name, profile, filtered_output)
        found_panics = _analyze_panic_patterns(filtered_output, verbose)
        return _report_panic_results(example_name, asm_file, found_panics)

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
        examples.extend(
            file[:-3]
            for file in os.listdir(examples_dir)
            if file.endswith('.rs')
        )
    return sorted(examples)

def run_panic_analysis(specific_examples=None):
    """Run panic checker on specified examples or all available ones."""
    examples = specific_examples or get_available_examples()
    results = {}

    print("\n=== Panic Reference Analysis ===")
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
    failed_count = sum("FAIL" in r for r in results.values())
    total_count = len([r for r in results.values() if r != "Not Found"])

    if failed_count > 0:
        print(f"\n❌ OVERALL: {failed_count}/{total_count} examples have panic references")
        return False
    else:
        print(f"\n✅ OVERALL: All {total_count} examples are panic-free!")
        return True

def _print_panic_usage():
    """Prints the usage instructions for the panic checker."""
    available = get_available_examples()
    print("Available examples for panic checking:")
    for example in available:
        print(f"  - {example}")
    print("\nUsage:")
    print("  python run_suite.py panic --example <name>                                    # Check specific example")
    print("  python run_suite.py panic --example <name> --no-default-features             # Check without default features")
    print("  python run_suite.py panic --example <name> --features depth-7,pico-tiny      # Check with specific features")
    print("  python run_suite.py panic --examples                                          # Check all examples")

def main():
    """Main entry point for the script."""
    parser = argparse.ArgumentParser(description="Run test suites for the picojson-rs crate.")
    parser.add_argument(
        "tool",
        nargs='?',
        default="bloat",
        choices=["bloat", "panic"],
        help="Run the remaining binary-size or panic-reference analysis. Stack campaigns use cargo krabi-caliper."
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

    if args.tool == "bloat":
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
            _print_panic_usage()
            sys.exit(0)

if __name__ == "__main__":
    main()
