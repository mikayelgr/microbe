def _mb_parts():
    import esp32, json
    P = esp32.Partition
    run = P(P.RUNNING)
    try:
        nxt = run.get_next_update().info()[4]
    except Exception:
        nxt = None
    out = []
    for t in (P.TYPE_APP, P.TYPE_DATA):
        for p in P.find(t):
            i = p.info()
            out.append([i[0], i[1], i[2], i[3], i[4], bool(i[5])])
    print(json.dumps({"partitions": out, "running": run.info()[4], "next_update": nxt}))


_mb_parts()
del _mb_parts
