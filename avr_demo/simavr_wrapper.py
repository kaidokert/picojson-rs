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
    parser = argparse.ArgumentParser(description="A wrapper for running simavr on Linux and Windows (via WSL).")
    parser.add_argument("-t", "--timeout", type=int, help="Timeout in seconds for the simulation.")
    parser.add_argument("binary", help="The ELF binary to simulate.")
    parser.add_argument("simavr_args", nargs=argparse.REMAINDER, help="Arguments to pass to simavr.")
    args = parser.parse_args()

    if not os.path.exists(args.binary):
        print(f"Error: Binary file '{args.binary}' not found.", file=sys.stderr)
        sys.exit(1)

    system = platform.system().lower()

    if system == "linux":
        exit_code = run_simavr_linux(args.binary, args.simavr_args, args.timeout)
    elif system == "windows":
        exit_code = run_simavr_windows(args.binary, args.simavr_args, args.timeout)
    else:
        print(f"Error: Unsupported operating system: {system}", file=sys.stderr)
        sys.exit(1)

    sys.exit(exit_code)

if __name__ == "__main__":
    main()
