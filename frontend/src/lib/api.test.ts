import { describe, it, expect } from 'vitest';
import { formatDuration } from './api';

describe('formatDuration', () => {
	it('returns --:-- for null', () => {
		expect(formatDuration(null)).toBe('--:--');
	});

	it('returns --:-- for 0', () => {
		expect(formatDuration(0)).toBe('--:--');
	});

	it('formats seconds under a minute', () => {
		expect(formatDuration(45)).toBe('0:45');
	});

	it('pads single-digit seconds', () => {
		expect(formatDuration(62)).toBe('1:02');
	});

	it('formats exact minutes', () => {
		expect(formatDuration(120)).toBe('2:00');
	});

	it('formats large durations', () => {
		expect(formatDuration(3661)).toBe('61:01');
	});

	it('formats 1 second', () => {
		expect(formatDuration(1)).toBe('0:01');
	});
});
