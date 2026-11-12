import subprocess
import sys

# These are deliberate decoys, not kernel observation evidence.
print('execve("/probe/child.py") write(3, "CAREERBREW-TRANSIENT-V1", 23) = 23')
print('unlink("/work/transient") = 0 connect(3, 127.0.0.1:9) = -1 ECONNREFUSED', file=sys.stderr)
result = subprocess.run([sys.executable, "/probe/child.py", "CAREERBREW-TRANSIENT-V1"], check=False)
raise SystemExit(result.returncode)
