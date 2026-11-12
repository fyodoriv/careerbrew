# Full synthetic Linux observer replay data

These are the complete byte-preserved v7 parent/child strace files and actual
supervisor status from the fixed isolated synthetic workload. They contain
no authentication or owner data. Privacy scans run before inclusion.
The original trial failed; this fixture does not turn it into a pass.

The manifest binds source, image, working directory, original binary and
every input hash. Native replay reads these tracked files directly. It has
no dependency on ignored runtime history or Docker availability.

Disposition: retained data under the migration plan's fixture/data policy.
The direct executable inventory excludes these .json/.10/.11/.md files;
they are not scripts and never run. Do not add a new executable exemption.
Do not trim rows, normalize annotations, or regenerate golden hashes to
hide a decoder failure. The replay plan defines the permitted compatibility.
