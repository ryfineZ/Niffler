#!/usr/bin/env python3
"""在隔离 Niffler 上固定一个 Codex OAuth 账号，比较遥测开关前后的客观题结果。"""
import argparse
import json
import os
import random
import signal
import time
import urllib.error
import urllib.request
from pathlib import Path

CONFIG_PATH = '/api/admin/system/configs/codex_telemetry_enabled'


def cases():
    def paths(n, m, blocked):
        table = [[0] * (m + 1) for _ in range(n + 1)]
        table[0][0] = 1
        for x in range(n + 1):
            for y in range(m + 1):
                if (x, y) in blocked:
                    table[x][y] = 0
                elif x or y:
                    table[x][y] = (table[x - 1][y] if x else 0) + (table[x][y - 1] if y else 0)
        return table[n][m]

    def count_strings(length):
        counts = {(0, 0): 1}
        for _ in range(length):
            next_counts = {}
            for (ones, trailing), count in counts.items():
                for digit in [0, 1]:
                    if digit and trailing == 2:
                        continue
                    key = (ones + digit, trailing + 1 if digit else 0)
                    next_counts[key] = next_counts.get(key, 0) + count
            counts = next_counts
        return sum(count for (ones, _), count in counts.items() if ones == 8)

    return [
        ('modular_power', '计算 37 的 12345 次方除以 1009 的余数。', pow(37, 12345, 1009)),
        ('blocked_grid', '从整数坐标 (0,0) 到 (9,8)，每步只能向右或向上走 1。不得经过 (2,3)、(5,4)、(7,6)。共有多少条路径？', paths(9, 8, {(2, 3), (5, 4), (7, 6)})),
        ('binary_strings', '长度为 19 的二进制字符串，恰有 8 个 1，且不含连续三个 1。共有多少个？', count_strings(19)),
        ('congruences', '求满足 x mod 17=9、x mod 19=7、x mod 23=15 的最小非负整数 x。', next(x for x in range(17 * 19 * 23) if x % 17 == 9 and x % 19 == 7 and x % 23 == 15)),
        ('subset_sum', '集合 {2,5,7,11,13,17,19,23,29,31} 有多少个元素和恰为 60 的子集？', sum(sum(v for i, v in enumerate([2, 5, 7, 11, 13, 17, 19, 23, 29, 31]) if mask >> i & 1) == 60 for mask in range(1 << 10))),
        ('digit_count', '从 1 到 2026（含首尾）的所有十进制整数中，数字 2 一共出现多少次？', sum(str(n).count('2') for n in range(1, 2027))),
    ]


def answer_text(response):
    if isinstance(response, str):
        return response
    if not isinstance(response, dict):
        return ''
    if isinstance(response.get('output_text'), str):
        return response['output_text']
    texts = []
    for item in response.get('output', []):
        for part in item.get('content', []):
            if isinstance(part.get('text'), str):
                texts.append(part['text'])
    if texts:
        return ''.join(texts)
    return ''.join(choice.get('message', {}).get('content', '') for choice in response.get('choices', []))


def grade(text, expected):
    stripped = text.strip()
    if stripped.startswith('```'):
        lines = stripped.splitlines()
        stripped = '\n'.join(lines[1:-1])
    try:
        value = json.loads(stripped)
    except (ValueError, TypeError):
        return False
    return isinstance(value, dict) and type(value.get('answer')) is int and value['answer'] == expected


class Api:
    def __init__(self, base, token):
        self.base, self.token = base.rstrip('/'), token
        self.opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

    def call(self, method, path, payload=None):
        request = urllib.request.Request(self.base + path,
            data=json.dumps(payload).encode() if payload is not None else None,
            method=method, headers={'Authorization': 'Bearer ' + self.token, 'Content-Type': 'application/json',
                'x-client-device-id': os.environ['NIFFLER_TEST_DEVICE_ID']})
        try:
            with self.opener.open(request, timeout=240) as response:
                return json.load(response)
        except urllib.error.HTTPError as error:
            raise RuntimeError(f'HTTP {error.code}') from None
        except (urllib.error.URLError, TimeoutError):
            raise RuntimeError('网络失败或超时') from None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--provider-id', required=True)
    parser.add_argument('--key-id', required=True)
    parser.add_argument('--model', required=True)
    parser.add_argument('--effort', default='high')
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    base, token = os.environ.get('NIFFLER_TEST_BASE_URL'), os.environ.get('NIFFLER_TEST_ADMIN_TOKEN')
    if not base or not token or not os.environ.get('NIFFLER_TEST_DEVICE_ID'):
        parser.error('需要 NIFFLER_TEST_BASE_URL、NIFFLER_TEST_ADMIN_TOKEN 和对应的 NIFFLER_TEST_DEVICE_ID；不要把令牌放进命令参数')
    api = Api(base, token)
    previous = api.call('GET', CONFIG_PATH)['value']
    if type(previous) is not bool:
        raise RuntimeError('遥测配置不是布尔值，停止实验')
    results = {'model': args.model, 'effort': args.effort, 'samples': [], 'restored': False,
        'limitations': ['小样本探索；没有盲测或独立账号随机分组。', '遥测对账号的持续作用未知，无法确保无残留影响。', '同题复测有缓存和时间顺序影响；接口成功不等于回答能力提高。']}

    def save():
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(results, ensure_ascii=False, indent=2) + '\n')

    def stop(*_):
        raise KeyboardInterrupt

    signal.signal(signal.SIGTERM, stop)
    try:
        for enabled in [False, True]:
            api.call('PUT', CONFIG_PATH, {'value': enabled})
            if enabled:
                # 预热一次，让启动事件和一分钟增量指标有机会完成上报。
                api.call('POST', '/api/admin/provider-query/test-model', {
                    'provider_id': args.provider_id, 'api_key_id': args.key_id,
                    'model_name': args.model, 'api_format': 'openai:responses', 'mode': 'direct',
                    'request_body': {'model': args.model, 'input': [{'role': 'user', 'content': 'Reply READY.'}],
                                     'reasoning': {'effort': args.effort}, 'stream': True}})
                time.sleep(65)
            ordered = list(cases())
            random.Random(20260916).shuffle(ordered)
            for name, prompt, expected in ordered:
                started = time.monotonic()
                sample = {'case': name, 'telemetry_enabled': enabled, 'expected': expected}
                try:
                    result = api.call('POST', '/api/admin/provider-query/test-model', {
                        'provider_id': args.provider_id, 'api_key_id': args.key_id,
                        'model_name': args.model, 'api_format': 'openai:responses', 'mode': 'direct',
                        'request_body': {'model': args.model, 'stream': True, 'store': False,
                            'reasoning': {'effort': args.effort}, 'tools': [],
                            'input': [{'role': 'user', 'content': prompt + ' 只返回 JSON 对象 {"answer":整数}。'}]}})
                    response = (result.get('data') or {}).get('response') or {}
                    text = answer_text(response)
                    sample.update(success=result.get('success') is True,
                        correct=result.get('success') is True and grade(text, expected), answer=text,
                        usage=response.get('usage', {}) if isinstance(response, dict) else {})
                except RuntimeError as error:
                    sample.update(success=False, correct=False, error=str(error))
                sample['elapsed_seconds'] = round(time.monotonic() - started, 3)
                results['samples'].append(sample)
                save()
                print(f'{"开启" if enabled else "关闭"} {name}: {"通过" if sample["correct"] else "未通过"}', flush=True)
        time.sleep(65)
    finally:
        try:
            api.call('PUT', CONFIG_PATH, {'value': previous})
            results['restored'] = True
        finally:
            save()
    for enabled in [False, True]:
        group = [sample for sample in results['samples'] if sample['telemetry_enabled'] == enabled]
        print(f'{"开启" if enabled else "关闭"}: {sum(sample["correct"] for sample in group)}/{len(group)}')
    print('该结果只作探索，不足以确认或否定能力改善。')


if __name__ == '__main__':
    try:
        main()
    except (RuntimeError, KeyboardInterrupt) as error:
        raise SystemExit(str(error) or '实验中断；已尝试恢复开关，请核对结果文件') from None
