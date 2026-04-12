<script lang="ts">
	import { page } from '$app/stores';
	import { onMount, onDestroy } from 'svelte';
	import {
		getFeed,
		submitEpisode,
		formatDuration,
		type FeedWithEpisodes,
		type Episode,
	} from '$lib/api';

	let feed = $state<FeedWithEpisodes | null>(null);
	let error = $state('');
	let submitUrl = $state('');
	let submitTts = $state('');
	let submitting = $state(false);

	let token = $derived($page.params.token ?? '');
	let pollInterval: ReturnType<typeof setInterval> | null = null;

	async function loadFeed() {
		try {
			feed = await getFeed(token);
			if (!submitTts && feed) {
				submitTts = feed.tts_default;
			}
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to load feed';
		}
	}

	function hasInProgress(episodes: Episode[]): boolean {
		return episodes.some(
			(ep) => !['done', 'error'].includes(ep.status)
		);
	}

	function startPolling() {
		stopPolling();
		pollInterval = setInterval(async () => {
			if (feed && hasInProgress(feed.episodes)) {
				await loadFeed();
			}
		}, 5000);
	}

	function stopPolling() {
		if (pollInterval) {
			clearInterval(pollInterval);
			pollInterval = null;
		}
	}

	onMount(async () => {
		await loadFeed();
		startPolling();
	});

	onDestroy(() => {
		stopPolling();
	});

	async function handleSubmit() {
		if (!submitUrl.trim()) return;
		submitting = true;
		error = '';
		try {
			await submitEpisode(token, submitUrl, submitTts || undefined);
			submitUrl = '';
			await loadFeed();
			startPolling();
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to submit episode';
		} finally {
			submitting = false;
		}
	}

	function copyToClipboard(text: string) {
		navigator.clipboard.writeText(text);
	}

	function badgeClass(status: string): string {
		if (status === 'done') return 'badge done';
		if (status === 'error') return 'badge error';
		if (status === 'pending') return 'badge pending';
		return 'badge processing';
	}
</script>

{#if feed}
	<div class="flex-between mb-2">
		<div>
			<h2>{feed.title}</h2>
			<p class="muted">{feed.description}</p>
		</div>
		<button class="copy-btn" onclick={() => feed && copyToClipboard(feed.rss_url)}>
			Copy RSS URL
		</button>
	</div>

	<div class="card mb-2">
		<form onsubmit={(e) => { e.preventDefault(); handleSubmit(); }}>
			<label class="mb-1" style="display:block; font-weight:500;">Submit URL</label>
			<div class="flex">
				<input
					bind:value={submitUrl}
					placeholder="https://arxiv.org/abs/2301.07041 or article URL"
					disabled={submitting}
				/>
				<select bind:value={submitTts} style="width: auto;">
					<option value="openai">OpenAI</option>
					<option value="elevenlabs">ElevenLabs</option>
				</select>
				<button type="submit" class="primary" disabled={submitting}>
					{submitting ? 'Submitting...' : 'Submit'}
				</button>
			</div>
		</form>
	</div>

	{#if error}
		<div class="card mb-2" style="border-color: var(--danger); color: var(--danger);">{error}</div>
	{/if}

	<h2>Episodes</h2>

	{#each feed.episodes as ep}
		<div class="card">
			<div class="flex-between mb-1">
				<a href="/feeds/{token}/episodes/{ep.id}">
					<strong>{ep.title}</strong>
				</a>
				<span class={badgeClass(ep.status)}>{ep.status}</span>
			</div>
			<div class="muted" style="font-size: 0.8rem;">
				<a href={ep.source_url} target="_blank" rel="noopener">{ep.source_url}</a>
			</div>
			{#if ep.status === 'error' && ep.error_msg}
				<div style="color: var(--danger); font-size: 0.85rem; margin-top: 0.5rem;">
					{ep.error_msg}
				</div>
			{/if}
			{#if ep.status === 'done' && ep.audio_url}
				<div class="mt-2 flex">
					<audio controls src={ep.audio_url} preload="none" style="height: 32px;"></audio>
					<span class="muted">{formatDuration(ep.duration_secs)}</span>
				</div>
			{/if}
		</div>
	{:else}
		<p class="muted">No episodes yet. Submit a URL above to create one.</p>
	{/each}
{:else if error}
	<div class="card" style="border-color: var(--danger); color: var(--danger);">{error}</div>
{:else}
	<p class="muted">Loading...</p>
{/if}
