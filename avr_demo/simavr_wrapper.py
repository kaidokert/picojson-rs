import platform
import subprocess
import sys
import os
import argparse
import shlex

def run_simavr_linux(binary, simavr_args, timeout_seconds=None):
    """Run simavr directly on Linux with all arguments passed through."""
    cmd = ["simavr"] + simavr_args + [binary]

    if timeout_seconds:
        cmd = ["timeout", str(timeout_seconds)] + cmd

    try:
        result = subprocess.run(cmd, check=False)
        return result.returncode
    except FileNotFoundError:
        print("Error: simavr not found. Please ensure it's installed and in PATH.", file=sys.stderr)
        return 1

def run_simavr_windows(binary, simavr_args, timeout_seconds=None):
    """Run simavr through WSL on Windows with path conversion."""
    try:
        abs_binary_path = os.path.abspath(binary)
        normalized_path = abs_binary_path.replace('\\', '/')

        wsl_binary_result = subprocess.run(
            ["wsl", "wslpath", "-u", normalized_path],
            capture_output=True,
            text=True,
            check=True
        )
        wsl_binary_path = wsl_binary_result.stdout.strip()

        simavr_cmd_parts = ["simavr"] + simavr_args + [wsl_binary_path]

        if timeout_seconds:
            simavr_cmd_parts = ["timeout", str(timeout_seconds)] + simavr_cmd_parts

        cmd = ["wsl", "-e", "bash", "-c", " ".join(shlex.quote(part) for part in simavr_cmd_parts)]
        result = subprocess.run(cmd, check=False)
        return result.returncode

    except subprocess.CalledProcessError as e:
        print(f"Error converting path with wslpath: {e}", file=sys.stderr)
        return 1
    except FileNotFoundError:
        print("Error: WSL not found. Please ensure WSL is installed and configured.", file=sys.stderr)
        return 1

def main():
    # Custom parsing needed because argparse can't handle mixed arguments properly
    # We need to extract -t/--timeout but pass everything else to simavr
    timeout_seconds = None
    simavr_args = []
    binary = None

    i = 1  # Skip script name
    while i < len(sys.argv):
        arg = sys.argv[i]

        if arg in ['-t', '--timeout']:
            if i + 1 >= len(sys.argv):
                print("Error: -t/--timeout requires a value", file=sys.stderr)
                sys.exit(1)
            try:
                timeout_seconds = int(sys.argv[i + 1])
                if timeout_seconds <= 0:
                    raise ValueError()
            except ValueError:
                print("Error: timeout value must be a positive integer", file=sys.stderr)
                sys.exit(1)
            i += 2  # Skip both the flag and its value
        elif arg.endswith('.elf') or arg.endswith('.bin') or arg.endswith('.hex'):
            # This looks like a binary file
            binary = arg
            i += 1
        else:
            # This is a simavr argument
            simavr_args.append(arg)
            i += 1

    # If no binary was found, assume the last argument is the binary
    if binary is None and simavr_args:
        binary = simavr_args.pop()

    if binary is None:
        print("Error: No binary file specified", file=sys.stderr)
        print("Usage: simavr_wrapper.py [-t timeout] [simavr_args...] <binary.elf>", file=sys.stderr)
        sys.exit(1)

    if not os.path.exists(binary):
        print(f"Error: Binary file '{binary}' not found.", file=sys.stderr)
        sys.exit(1)

    system = platform.system().lower()

    if system == "linux":
        exit_code = run_simavr_linux(binary, simavr_args, timeout_seconds)
    elif system == "windows":
        exit_code = run_simavr_windows(binary, simavr_args, timeout_seconds)
    else:
        print(f"Error: Unsupported operating system: {system}", file=sys.stderr)
        sys.exit(1)

    sys.exit(exit_code)

if __name__ == "__main__":
    main()
