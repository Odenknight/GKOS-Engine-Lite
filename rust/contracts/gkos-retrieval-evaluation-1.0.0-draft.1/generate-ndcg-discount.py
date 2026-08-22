from decimal import Decimal, ROUND_HALF_UP, localcontext
import hashlib
import json
import sys

SCALE = Decimal(1_000_000_000_000)

with localcontext() as context:
    context.prec = 80
    values = [
        int((SCALE * Decimal(2).ln() / Decimal(rank + 1).ln()).quantize(Decimal(1), rounding=ROUND_HALF_UP))
        for rank in range(1, 101)
    ]

material = {
    "contract_version": "gkos-retrieval-evaluation-ndcg-table/1.0.0-draft.1",
    "ndcg_discount_scale": 1_000_000_000_000,
    "rank_count": 100,
    "ndcg_discount_scaled": values,
    "generator": {
        "arithmetic": "decimal",
        "precision": 80,
        "rounding": "ROUND_HALF_UP",
        "formula": "ndcg_discount_scale * ln(2) / ln(rank + 1)",
    },
}
canonical = json.dumps(material, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
envelope = {**material, "table_digest": "sha256:" + hashlib.sha256(canonical.encode("utf-8")).hexdigest()}
sys.stdout.buffer.write((json.dumps(envelope, ensure_ascii=False, indent=2) + "\n").encode("utf-8"))
