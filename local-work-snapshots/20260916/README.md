# Local work snapshot

Development backup; qualification is false. Restore each checkout independently from its parent and exact INDEX layers. Working deletions differ from staged deletions; index modes and stages are retained. Archives hold raw bytes with fixed metadata, sorted content addresses and gzip mtime 0.

Private backup pointers preserve omissions from public repositories. Their binding hashes refer to full original-owner, checkout, path and layer records in the private GKOS-Engine-Rust backup. These records are backups only, never merged product source. No original file was redacted or modified.
