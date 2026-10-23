"""Compare production sanma features and recommendations to Akagi's libriichi3p.

Reference binaries and weights must be supplied locally; neither is distributed.
Uses deterministic generated positions, not an estimate of match win rate.
"""
import argparse
import collections
import hashlib
import importlib.util
import json
import logging
from pathlib import Path
import random
import subprocess
import sys
import types

import numpy as np

REFERENCE_BINARY_SHA256='0a0b295e6954c7fe85765b69519c8bcfd31af19c224281b203f9c20672962523'
REFERENCE_MODEL_SHA256='25dc6e88cd747c66dcff9088c1902560112f2c059327957b43fa35f10ce47436'


def sha(path):
    with path.open('rb') as f: return hashlib.file_digest(f,'sha256').hexdigest()


def scenarios(count, seed=20261024):
    rng = random.Random(seed)
    suits = ['1m', '9m'] + [f'{i}{s}' for s in 'ps' for i in range(1, 10)] + list('ESWNPFC')
    wall_template = [t for t in suits for _ in range(4)]
    wall_template[wall_template.index('5p')] = '5pr'
    wall_template[wall_template.index('5s')] = '5sr'
    cases = []

    def start(hand, seat=0, dealer=0, scores=None, wind='E', kyoku=1, marker='9s'):
        hands = [['?']*13 for _ in range(4)]
        hands[seat] = hand.copy()
        return [dict(type='start_game', names=['A','B','C','']),
                dict(type='start_kyoku', bakaze=wind, dora_marker=marker, kyoku=kyoku,
                     honba=0, kyotaku=0, oya=dealer, scores=(scores or [35000]*3)+[0], tehais=hands)]

    for i in range(count):
        wall = wall_template.copy(); rng.shuffle(wall)
        hand, draws = wall[:13], wall[14:]
        seat = i % 3
        events = start(hand, seat, seat, [35000]*3 if i % 2 else [18000,42000,45000],
                       'S' if i % 3 == 0 else 'E', 1+i%3, wall[13])
        for turn in range(1+(i%6)):
            tile = draws.pop()
            events.append(dict(type='tsumo', actor=seat, pai=tile))
            if turn == i%6:
                break
            events.append(dict(type='dahai', actor=seat, pai=tile, tsumogiri=True))
            for offset in [1,2]:
                other=(seat+offset)%3
                tile=draws.pop()
                events.extend([dict(type='tsumo',actor=other,pai='?'),
                               dict(type='dahai',actor=other,pai=tile,tsumogiri=True)])
        cases.append(dict(id=f'random-{i:03}',group='draw',seat=seat,events=events))

    base=['1p','2p','3p','4p','5p','6p','7s','8s','9s','P','P','F','C']
    def add(name, hand, tail, group, seat=0, dealer=0):
        cases.append(dict(id=name,group=group,seat=seat,events=start(hand,seat,dealer)+tail))
    add('kita',base[:-1]+['N'],[dict(type='tsumo',actor=0,pai='9m')],'kita')
    add('ankan',['1p']*3+base[3:],[dict(type='tsumo',actor=0,pai='1p')],'kan')
    tenpai=['1p','2p','3p','4p','5p','6p','7s','8s','9s','P','P','P','E']
    add('tsumo',tenpai,[dict(type='tsumo',actor=0,pai='E')],'win')
    add('riichi',tenpai,[dict(type='tsumo',actor=0,pai='9m')],'riichi')
    add('ron',tenpai,[dict(type='tsumo',actor=1,pai='?'),
                     dict(type='dahai',actor=1,pai='E',tsumogiri=True)],'win',dealer=1)
    add('pon',base,[dict(type='tsumo',actor=1,pai='?'),
                   dict(type='dahai',actor=1,pai='P',tsumogiri=True)],'pon',dealer=1)
    add('kakan',['P']*2+base[:9]+['F','C'],[
        dict(type='tsumo',actor=1,pai='?'),dict(type='dahai',actor=1,pai='P',tsumogiri=True),
        dict(type='pon',actor=0,target=1,pai='P',consumed=['P','P']),
        dict(type='dahai',actor=0,pai='F',tsumogiri=False),
        dict(type='tsumo',actor=1,pai='?'),dict(type='dahai',actor=1,pai='9m',tsumogiri=True),
        dict(type='tsumo',actor=2,pai='?'),dict(type='dahai',actor=2,pai='1m',tsumogiri=True),
        dict(type='tsumo',actor=0,pai='P')],'kan',dealer=1)
    add('opponent-riichi',base,[
        dict(type='tsumo',actor=1,pai='?'),dict(type='reach',actor=1),
        dict(type='dahai',actor=1,pai='9m',tsumogiri=True),
        dict(type='reach_accepted',actor=1),
        dict(type='tsumo',actor=2,pai='?'),dict(type='dahai',actor=2,pai='1m',tsumogiri=True),
        dict(type='tsumo',actor=0,pai='N')],'riichi',dealer=1)
    add('own-riichi',tenpai,[
        dict(type='tsumo',actor=0,pai='9m'),dict(type='reach',actor=0),
        dict(type='dahai',actor=0,pai='9m',tsumogiri=True),dict(type='reach_accepted',actor=0),
        dict(type='tsumo',actor=1,pai='?'),dict(type='dahai',actor=1,pai='1m',tsumogiri=True),
        dict(type='tsumo',actor=2,pai='?'),dict(type='dahai',actor=2,pai='9m',tsumogiri=True),
        dict(type='tsumo',actor=0,pai='N')],'riichi')
    return cases


class Capture:
    engine_type='mortal'; name='quality-audit'; is_oracle=False; version=4
    enable_quick_eval=False; enable_rule_based_agari_guard=False
    def __init__(self): self.last=None
    def react_batch(self, observations, masks, invisible):
        # The reference prepends a separate kan-selection item when applicable.
        # Its final batch item is the main action-selection observation.
        self.last=(np.array(observations[-1],dtype=np.float32),np.array(masks[-1],dtype=bool))
        actions=[43 if m[43] else int(np.flatnonzero(m)[0]) for m in masks]
        return actions, [[0.0]*44 for _ in masks], [m.tolist() for m in masks], [True]*len(masks)


def reference_network(folder, checkpoint):
    import torch
    import libriichi3p
    package=types.ModuleType('quality_reference'); package.__path__=[str(folder)]
    sys.modules['quality_reference']=package
    sys.modules['quality_reference.libriichi3p']=libriichi3p
    sys.modules['quality_reference.libriichi3p.mjai']=libriichi3p.mjai
    sys.modules['quality_reference.libriichi3p.consts']=libriichi3p.consts
    logger=types.ModuleType('quality_reference.logger'); logger.logger=logging.getLogger('quality')
    sys.modules['quality_reference.logger']=logger
    spec=importlib.util.spec_from_file_location('quality_reference.model',folder/'reference_model.py')
    module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
    weights=torch.load(checkpoint,map_location='cpu',weights_only=True)
    config=weights['config']
    brain=module.Brain(version=4,conv_channels=config['resnet']['conv_channels'],num_blocks=config['resnet']['num_blocks']).eval()
    dqn=module.DQN(version=4).eval()
    brain.load_state_dict(weights['mortal'],strict=True);dqn.load_state_dict(weights['current_dqn'],strict=True)
    return brain,dqn


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--reference-dir',type=Path,required=True)
    p.add_argument('--checkpoint',type=Path,required=True)
    p.add_argument('--replay-exe',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--samples',type=int,default=120)
    p.add_argument('--seed',type=int,default=20261024)
    p.add_argument('--cases',type=Path,help='Optional independent MJAI replay cases instead of generated positions')
    p.add_argument('--expected-reference-sha256',default=REFERENCE_BINARY_SHA256)
    p.add_argument('--diagnose-sp',action='store_true',help='Offline ablation: substitute reference SP planes; never changes the app')
    args=p.parse_args();args.output.mkdir(parents=True,exist_ok=True)
    if args.samples < 0: p.error('--samples must not be negative')
    if sha(args.reference_dir/'libriichi3p.pyd') != args.expected_reference_sha256:
        p.error('Reference binary hash mismatch; use a verified pinned binary')
    if sha(args.reference_dir/'reference_model.py') != REFERENCE_MODEL_SHA256:
        p.error('Reference model definition does not match the pinned Akagi source')
    sys.path.insert(0,str(args.reference_dir.resolve()))
    import libriichi3p
    import torch
    assert libriichi3p.consts.obs_shape(4)==(775,34) and libriichi3p.consts.ACTION_SPACE==44
    torch.set_num_threads(4)
    cases=json.loads(args.cases.read_text(encoding='utf-8')) if args.cases else scenarios(args.samples,args.seed)
    refs=[]; valid=[]; errors=[]
    for case in cases:
        capture=Capture();bot=libriichi3p.mjai.Bot(capture,case['seat'])
        try:
            for event in case['events']:
                capture.last=None
                bot.react(json.dumps(event,separators=(',',':')))
            if capture.last is None: raise ValueError('final event has no inference')
            valid.append(case);refs.append(capture.last)
        except Exception as ex: errors.append(dict(id=case['id'],error=str(ex)))
    cases_path=args.output/'cases.json';rust_path=args.output/'production-observations.json'
    cases_path.write_text(json.dumps(valid),encoding='utf-8')
    subprocess.run([str(args.replay_exe.resolve()),str(cases_path),str(rust_path)],check=True)
    production=json.loads(rust_path.read_text(encoding='utf-8'))
    np.savez_compressed(args.output/'reference-observations.npz',
                        obs=np.stack([x[0] for x in refs]),mask=np.stack([x[1] for x in refs]))
    brain,dqn=reference_network(args.reference_dir,args.checkpoint)
    metrics=[]; different_channels=collections.Counter()
    for case,(ref,ref_mask),local in zip(valid,refs,production):
        obs=np.array(local['obs'],np.float32).reshape(775,34);mask=np.array(local['mask'],bool)
        delta=np.abs(obs-ref)
        for channel in np.flatnonzero(np.any(delta>1e-6,axis=1)): different_channels[int(channel)]+=1
        with torch.inference_mode():
            observations=[ref,obs]
            if args.diagnose_sp:
                hybrid=obs.copy();hybrid[652:]=ref[652:]
                observations.append(hybrid)
            both=torch.from_numpy(np.stack(observations))
            # Equal reference mask isolates feature errors from action-codec errors.
            q=dqn(brain(both),torch.from_numpy(np.stack([ref_mask]*len(observations)))).numpy()
            deployed=dqn(brain(torch.from_numpy(obs[None])),torch.from_numpy(mask[None])).numpy()[0]
        reference_action=int(q[0].argmax());feature_action=int(q[1].argmax());actual_action=int(deployed.argmax())
        codec=local['specialActionIndices']
        to_reference={codec['pon']:38,codec['kan']:39,codec['kita']:40}
        semantic_action=to_reference.get(actual_action,actual_action)
        legal=np.flatnonzero(ref_mask)
        metrics.append(dict(id=case['id'],group=case['group'],
            maskEqual=bool(np.array_equal(mask,ref_mask)),referenceMask=legal.tolist(),localMask=np.flatnonzero(mask).tolist(),
            differingChannels=int(np.count_nonzero(np.any(delta>1e-6,axis=1))),
            differingValues=int(np.count_nonzero(delta>1e-6)),maxFeatureError=float(delta.max()),
            referenceAction=reference_action,featureOnlyAction=feature_action,localAction=actual_action,
            localCanonicalAction=semantic_action,
            featureTop1=reference_action==feature_action,totalTop1=reference_action==semantic_action,
            referenceRegret=float(q[0,reference_action]-q[0,semantic_action]) if ref_mask[semantic_action] else None,
            legalQMaxError=float(np.max(np.abs(q[0,legal]-q[1,legal])))))
        if args.diagnose_sp:
            metrics[-1]['referenceSpAction']=int(q[2].argmax())
            metrics[-1]['referenceSpTop1']=int(q[2].argmax())==reference_action
    n=len(metrics)
    report=dict(samples=n,seed=args.seed,skipped=errors,
        checkpointSha256=sha(args.checkpoint),referenceBinarySha256=sha(args.reference_dir/'libriichi3p.pyd'),
        referenceModelSha256=sha(args.reference_dir/'reference_model.py'),
        exactFeaturePositions=sum(m['differingValues']==0 for m in metrics),
        maskAgreement=sum(m['maskEqual'] for m in metrics),
        featureTop1Agreement=sum(m['featureTop1'] for m in metrics),
        totalTop1Agreement=sum(m['totalTop1'] for m in metrics),
        differingChannels=dict(sorted(different_channels.items())),cases=metrics,
        note='Consistency with a fixed reference, not win rate or proof of optimal play.')
    if args.diagnose_sp:
        report['offlineReferenceSpAgreement']=sum(m['referenceSpTop1'] for m in metrics)
        report['offlineReferenceSpNote']='Diagnostic substitution only; the Android app does not have these reference SP planes.'
    (args.output/'report.json').write_text(json.dumps(report,indent=2,allow_nan=False),encoding='utf-8')
    print(json.dumps({k:v for k,v in report.items() if k not in ['cases','differingChannels']},indent=2))
    print('Most frequent differing channels:',different_channels.most_common(18))


if __name__=='__main__': main()
