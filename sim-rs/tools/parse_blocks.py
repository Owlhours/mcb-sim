#!/usr/bin/env python3
"""Parses the decompiled Blocks.java: block field -> (registry name, class, properties text)."""
import re, sys, json
SRC='/work/reference/mc/decompiled/net/minecraft/world/level/block/Blocks.java'
def parse():
    txt=open(SRC).read()
    # helper functions that create blocks (private static Block registerXxx / xxx(...))
    out={}
    for m in re.finditer(r'public static final Block (\w+) = ', txt):
        name=m.group(1); i=m.end(); depth=0; j=i
        while True:
            ch=txt[j]
            if ch=='(': depth+=1
            elif ch==')':
                depth-=1
            elif ch==';' and depth==0: break
            j+=1
        body=txt[i:j]
        out[name]=body
    return out, txt
if __name__=='__main__':
    out,txt=parse()
    import collections
    heads=collections.Counter(re.match(r'\w+',b).group(0) for b in out.values())
    print(heads)
    cls=collections.Counter()
    for n,b in out.items():
        m=re.search(r'new (\w+)\(',b) or re.search(r'(\w+)::new',b)
        cls[m.group(1) if m else 'Block']+=1
    print(len(cls)); print(cls)
    # helper function definitions
    for m in re.finditer(r'private static \w+ (\w+)\(([^)]*)\) \{', txt): print(m.group(1), m.group(2)[:100])
