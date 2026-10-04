#!/usr/bin/env python3
"""Exhaustive small certified forests: original ancestry scan vs stackless DFS."""
from itertools import product
from collections import Counter
from z3 import Ints, Solver, Not, sat, unsat

def reference(parent, root):
    out = [(root, 0), (root, 1)]
    for node in range(len(parent)):
        if node == root: continue
        at = node
        while parent[at] is not None:
            p, arm = parent[at]
            if p == root:
                out.extend([(node, arm), (node, arm)])
                break
            at = p
    return Counter(out)

def walk(parent, children, root):
    out=[]
    for root_arm in range(2):
        out.append((root,root_arm))
        current=children.get((root,root_arm))
        if current is None: continue
        next_arm=0
        steps=0
        while True:
            steps+=1
            assert steps <= 4*len(parent)+1
            if next_arm<2:
                arm=next_arm
                next_arm+=1
                out.append((current,root_arm))
                child=children.get((current,arm))
                if child is not None:
                    current=child
                    next_arm=0
            else:
                p,arm=parent[current]
                if p==root: break
                current=p
                next_arm=arm+1
    return Counter(out)

forests=queries=0
for n in range(1,8):
    choices=[[None]+[(p,a) for p in range(i) for a in range(2)] for i in range(n)]
    for parents in product(*choices):
        used=[p for p in parents if p is not None]
        if len(used)!=len(set(used)): continue
        children={p:i for i,p in enumerate(parents) if p is not None}
        forests+=1
        for root in range(n):
            assert walk(parents,children,root)==reference(parents,root)
            queries+=1
print(f"Exact visitor multisets: {forests} certified forests, {queries} root queries.")

# Inductive rank facts for termination; explicit missing-premise counterexample.
parent,child,n=Ints('parent child n')
s=Solver();s.add(0<=parent,parent<child,child<n,Not(n-child<n-parent))
assert s.check()==unsat
print("forward-child rank strictly decreases: unsat")
s=Solver();s.add(0<=parent,parent<child,child<n,Not(parent<child))
assert s.check()==unsat
print("parent unwind strictly decreases ordinal: unsat")
s=Solver();s.add(parent==child,0<=child,child<n)
assert s.check()==sat
print("omitting strict-child certificate permits self-cycle: sat")
