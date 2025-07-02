import platform
import subprocess
import sys
import os
import argparse

def run_simavr_linux(binary, simavr_args, timeout_seconds=None):
    """Run simavr directly on Linux with all arguments passed through."""
    cmd = ["simavr"] + simavr_args + [binary]

    # Add timeout command if specified
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
        # Convert to absolute path first, then normalize separators for WSL
        abs_binary_path = os.path.abspath(binary)
        # wslpath requires forward slashes in input
        normalized_path = abs_binary_path.replace('\\', '/')

        # Convert Windows absolute path to WSL path using wslpath
        wsl_binary_result = subprocess.run(
            ["wsl", "wslpath", "-u", normalized_path],
            capture_output=True,
            text=True,
            check=True
        )
        wsl_binary_path = wsl_binary_result.stdout.strip()

        print(f"Windows path: {abs_binary_path}")
        print(f"Normalized path: {normalized_path}")
        print(f"WSL path: {wsl_binary_path}")

        # Build simavr command with args and binary path
        simavr_cmd = f"simavr {' '.join(simavr_args)} '{wsl_binary_path}'"

        # Add timeout command if specified
        if timeout_seconds:
            simavr_cmd = f"timeout {timeout_seconds} {simavr_cmd}"

        cmd = ["wsl", "-e", "bash", "-c", simavr_cmd]
        print(f"Running command: {cmd}")
        result = subprocess.run(cmd, check=False)
        return result.returncode

    except subprocess.CalledProcessError as e:
        print(f"Error converting path with wslpath: {e}", file=sys.stderr)
        return 1
    except FileNotFoundError:
        print("Error: WSL not found. Please ensure WSL is installed and configured.", file=sys.stderr)
        return 1

def parse_arguments():
    """Parse command line arguments, handling timeout and simavr args."""
    # We need custom parsing because we want to extract -t/--timeout but pass everything else to simavr
    timeout_seconds = None
    simavr_args = []
    binary = None

    i = 1  # Skip script name
    while i < len(sys.argv):
        arg = sys.argv[i]

        if arg in ['-t', '--timeout']:
            # Next argument should be the timeout value
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
            # This looks like a binary file, treat it as the target
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
        print("Usage: avrsim_wrapper.py [-t timeout] [simavr_args...] <binary.elf>", file=sys.stderr)
        sys.exit(1)

    return binary, simavr_args, timeout_seconds

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: avrsim_wrapper.py [-t timeout] [simavr_args...] <binary.elf>", file=sys.stderr)
        print("Examples:", file=sys.stderr)
        print("  avrsim_wrapper.py -m atmega2560 -f 16000000 firmware.elf", file=sys.stderr)
        print("  avrsim_wrapper.py -t 5 -m atmega2560 -f 16000000 firmware.elf", file=sys.stderr)
        sys.exit(1)

    # Parse arguments
    binary, simavr_args, timeout_seconds = parse_arguments()

    print(f"Binary: {binary}")
    print(f"Simavr args: {simavr_args}")
    if timeout_seconds:
        print(f"Timeout: {timeout_seconds} seconds")

    # Check if binary file exists
    if not os.path.exists(binary):
        print(f"Error: Binary file '{binary}' not found.", file=sys.stderr)
        sys.exit(1)

    # Detect operating system and run appropriate command
    system = platform.system().lower()

    if system == "linux":
        print(f"Running simavr on Linux: {binary}")
        exit_code = run_simavr_linux(binary, simavr_args, timeout_seconds)
    elif system == "windows":
        print(f"Running simavr on Windows via WSL: {binary}")
        exit_code = run_simavr_windows(binary, simavr_args, timeout_seconds)
    else:
        print(f"Error: Unsupported operating system: {system}", file=sys.stderr)
        print("This script supports Linux and Windows only.", file=sys.stderr)
        sys.exit(1)

    sys.exit(exit_code)
