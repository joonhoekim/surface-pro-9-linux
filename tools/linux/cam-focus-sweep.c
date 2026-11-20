// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Joonhoe Kim
/*
 * cam-focus-sweep: sharpness of the raw image against the lens position.
 *
 * usage: cam-focus-sweep VIDEO_NODE LENS_SUBDEV FROM TO STEP [DUMP_PREFIX]
 *
 * The pipeline must be set up first (cam-raw.sh does it; exposure and gain stay as they are).
 * Streams 10-bit unpacked frames, moves the lens from FROM to TO and back in STEP (focus_absolute
 * units), waits three frames after each move and prints, for the fourth, the mean and two sharpness
 * figures of the green pixels in the central half: the squared difference of neighbouring green
 * pairs four pixels apart (what the libcamera patch uses) and of single green pixels two apart
 * (finer detail), both divided by mean². With DUMP_PREFIX the frame of the sharpest position on the
 * way up is written to DUMP_PREFIX-<position>.raw (16-bit little endian, for cam-raw-stats.py).
 *
 * Build: cc -O2 -o cam-focus-sweep cam-focus-sweep.c
 */
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <unistd.h>
#include <linux/videodev2.h>

#define NBUF 4
#define SETTLE 3

static void die(const char *what)
{
	perror(what);
	exit(1);
}

int main(int argc, char **argv)
{
	if (argc < 6) {
		fprintf(stderr, "usage: %s VIDEO LENS_SUBDEV FROM TO STEP [DUMP_PREFIX]\n", argv[0]);
		return 2;
	}

	int vfd = open(argv[1], O_RDWR);
	int lfd = open(argv[2], O_RDWR);
	int from = atoi(argv[3]), to = atoi(argv[4]), step = abs(atoi(argv[5]));
	const char *prefix = argc > 6 ? argv[6] : NULL;
	if (vfd < 0 || lfd < 0)
		die("open");
	if (!step || from == to)
		return 2;

	struct v4l2_format fmt = { .type = V4L2_BUF_TYPE_VIDEO_CAPTURE };
	if (ioctl(vfd, VIDIOC_G_FMT, &fmt) < 0)
		die("VIDIOC_G_FMT");
	unsigned int w = fmt.fmt.pix.width, h = fmt.fmt.pix.height, stride = fmt.fmt.pix.bytesperline / 2;

	struct v4l2_requestbuffers req = { .count = NBUF, .type = V4L2_BUF_TYPE_VIDEO_CAPTURE, .memory = V4L2_MEMORY_MMAP };
	if (ioctl(vfd, VIDIOC_REQBUFS, &req) < 0)
		die("VIDIOC_REQBUFS");

	void *map[NBUF];
	for (unsigned int i = 0; i < req.count; i++) {
		struct v4l2_buffer buf = { .type = req.type, .memory = req.memory, .index = i };
		if (ioctl(vfd, VIDIOC_QUERYBUF, &buf) < 0)
			die("VIDIOC_QUERYBUF");
		map[i] = mmap(NULL, buf.length, PROT_READ, MAP_SHARED, vfd, buf.m.offset);
		if (map[i] == MAP_FAILED)
			die("mmap");
		if (ioctl(vfd, VIDIOC_QBUF, &buf) < 0)
			die("VIDIOC_QBUF");
	}

	int n = abs(to - from) / step + 1;
	int total = 2 * n - 1;
	uint16_t *best_frame = malloc((size_t)w * h * 2);
	double best = -1;
	int best_position = from;

	struct v4l2_control c = { .id = V4L2_CID_FOCUS_ABSOLUTE, .value = from };
	if (ioctl(lfd, VIDIOC_S_CTRL, &c) < 0)
		die("VIDIOC_S_CTRL");

	int type = V4L2_BUF_TYPE_VIDEO_CAPTURE;
	if (ioctl(vfd, VIDIOC_STREAMON, &type) < 0)
		die("VIDIOC_STREAMON");

	for (int i = 0, frame = 0; i < total; frame++) {
		struct v4l2_buffer buf = { .type = req.type, .memory = req.memory };
		if (ioctl(vfd, VIDIOC_DQBUF, &buf) < 0)
			die("VIDIOC_DQBUF");

		/* The first position gets extra frames: stream start and a long first move. */
		int wait = i == 0 ? 3 * SETTLE : SETTLE;
		static int waited;
		if (waited++ < wait) {
			if (ioctl(vfd, VIDIOC_QBUF, &buf) < 0)
				die("VIDIOC_QBUF");
			continue;
		}
		waited = 0;

		int k = i < n ? i : total - 1 - i;
		int position = from + (to > from ? 1 : -1) * k * step;
		const uint16_t *p = map[buf.index];
		double coarse = 0, fine = 0, sum = 0;
		unsigned long count = 0;

		/* Green pixels of one Bayer site: odd columns of the even rows or the reverse, either way one colour. */
		for (unsigned int y = h / 4; y + 4 < 3 * h / 4; y += 2) {
			const uint16_t *l = p + (size_t)y * stride;
			const uint16_t *b = l + 4 * stride;
			for (unsigned int x = (w / 4) | 1; x + 8 < 3 * w / 4; x += 4, count++) {
				int a = l[x] + l[x + 2];
				int ch = a - (l[x + 4] + l[x + 6]);
				int cv = a - (b[x] + b[x + 2]);
				int fh = l[x] - l[x + 2];
				int fv = l[x] - l[x + 2 * stride];
				coarse += (ch * ch + cv * cv) / 4.0;
				fine += fh * fh + fv * fv;
				sum += a / 2.0;
			}
		}
		double mean = sum / count;
		coarse /= 2.0 * count * mean * mean;
		fine /= 2.0 * count * mean * mean;
		printf("%s position %4d mean %6.1f coarse %.6f fine %.6f%s\n", i < n ? "up  " : "down", position, mean,
		       coarse, fine, buf.flags & V4L2_BUF_FLAG_ERROR ? " ERROR" : "");
		if (i < n && coarse > best) {
			best = coarse;
			best_position = position;
			for (unsigned int y = 0; y < h; y++)
				memcpy(best_frame + (size_t)y * w, p + (size_t)y * stride, w * 2);
		}

		if (ioctl(vfd, VIDIOC_QBUF, &buf) < 0)
			die("VIDIOC_QBUF");

		if (++i < total) {
			k = i < n ? i : total - 1 - i;
			c.value = from + (to > from ? 1 : -1) * k * step;
			if (ioctl(lfd, VIDIOC_S_CTRL, &c) < 0)
				die("VIDIOC_S_CTRL");
		}
	}

	ioctl(vfd, VIDIOC_STREAMOFF, &type);
	printf("sharpest on the way up: %d\n", best_position);

	if (prefix) {
		char name[512];
		snprintf(name, sizeof(name), "%s-%d.raw", prefix, best_position);
		FILE *f = fopen(name, "wb");
		if (!f)
			die(name);
		fwrite(best_frame, 2, (size_t)w * h, f);
		fclose(f);
		printf("wrote %s (%ux%u)\n", name, w, h);
	}
	return 0;
}
