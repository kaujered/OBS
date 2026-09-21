import openpyxl, math

BAR_PER_KGF = 0.980665
def to_bar(v): return v * BAR_PER_KGF
def to_kgf(v): return v / BAR_PER_KGF

def jones_bhp(ppl_kgf, a, b, q_mln):
    ppl_bar = to_bar(ppl_kgf)
    bsq = ppl_bar**2 - a*q_mln**2 - b*q_mln
    if bsq < 0: return None
    return to_kgf(math.sqrt(bsq))

def r2_standard(actuals, predictions):
    if len(actuals) < 2: return None
    mean_a = sum(actuals)/len(actuals)
    sse = sum((a-p)**2 for a,p in zip(actuals,predictions))
    sst = sum((a-mean_a)**2 for a in actuals)
    if sst < 1e-15: return 1.0 if sse < 1e-15 else None
    return 1.0 - sse/sst

wb = openpyxl.load_workbook("Книга2.xlsx", data_only=True)
ws = wb.active
rows = list(ws.iter_rows(values_only=True))

data = {}
well_meta = {}
for r in rows[1:]:
    if r[0] is None: continue
    well = r[0]
    thp, flo, bhp, pst, ppl = r[1], r[2], r[3], r[4], r[5]
    a_val, b_val = r[7], r[8]
    if well not in data:
        data[well] = []
        well_meta[well] = {"a": None, "b": None, "ppl": None}
    if ppl is not None: well_meta[well]["ppl"] = ppl
    if a_val is not None: well_meta[well]["a"] = a_val
    if b_val is not None: well_meta[well]["b"] = b_val
    if flo is not None and bhp is not None:
        data[well].append({"flo": flo, "bhp": bhp, "thp": thp, "ppl": ppl})

for well in sorted(data.keys()):
    pts = data[well]
    a = well_meta[well]["a"]
    b = well_meta[well]["b"]
    ppl = well_meta[well]["ppl"]
    if a is None or b is None or ppl is None: continue

    valid = [p for p in pts if None not in (p["flo"], p["thp"], p["bhp"], p["ppl"])
             and p["flo"]>=0 and p["thp"]>=1 and p["bhp"]>=1 and p["ppl"]>=1
             and p["bhp"]<=p["ppl"] and p["thp"]<=p["bhp"]]

    if len(valid) < 2: continue

    flo_mln = [p["flo"]/1e6 for p in valid]
    bhp_kgf = [p["bhp"] for p in valid]

    # --- OLD: bhp space, no anchor ---
    pred_no_anchor = [jones_bhp(ppl, a, b, q) for q in flo_mln]
    ok = [(act, pred) for act, pred in zip(bhp_kgf, pred_no_anchor) if pred is not None]
    r2_old = r2_standard([x[0] for x in ok], [x[1] for x in ok]) if len(ok)>=2 else None

    # --- NEW: bhp space, WITH anchor (0, ppl) ---
    all_bhp = [ppl] + bhp_kgf
    all_q   = [0.0] + flo_mln
    pred_with_anchor = [jones_bhp(ppl, a, b, q) for q in all_q]
    if None in pred_with_anchor:
        r2_new = None
    else:
        r2_new = r2_standard(all_bhp, pred_with_anchor)

    print("Well %d  a=%.2f b=%.2f ppl=%.2f  n=%d" % (well, a, b, ppl, len(valid)))
    print("  OLD (no anchor, bhp space): %.4f" % (r2_old if r2_old is not None else float('nan')))
    print("  NEW (with anchor, bhp space): %.4f" % (r2_new if r2_new is not None else float('nan')))
    print()
